//! 焼かれたタイルから標高を引く層。
//!
//! # 位置づけ
//!
//! [`dem::io`] がファイル 1 枚の読み書きを担当するのに対し、ここは
//! **「この測地座標の標高は？」に答える**。タイルの選択・読み込み・キャッシュを隠す。
//!
//! ```text
//!   Geodetic ──> どのタイルか ──> キャッシュ or 読み込み ──> バイリニア補間 ──> Meters
//! ```
//!
//! # 深いレベルから順に探す
//!
//! 同じ場所を複数のレベルで焼いてある場合、**細かいものを優先する**。
//! 見つからなければ 1 段粗いレベルへ落ちる。全 primary レベルを探した後だけ、
//! 供給元が持つ全球 fallback を使う。どちらにも無ければ `None` を返し、
//! 「海上」の判断は呼び出し側に委ねる（[ADR-0006](../../../../docs/adr/0006-simulation-integration-layer.md)）。
//!
//! [`dem::io`]: crate::dem::io

use crate::dem::DemTile;
use crate::dem::io::{MAX_TILE_FILE_BYTES, TileReadError, read_tile, tile_relative_path};
use crate::streaming::TileCache;
use crate::tile::{MAX_LEVEL, TileId};
use flightsim_core::{Geodetic, Meters};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// タイルの供給元。
///
/// ディスク以外（メモリ、アーカイブ、将来のネットワーク）も同じ形で挿せるように
/// トレイトにしてある。テストはメモリ実装を使う。
pub trait TileSource {
    /// タイルを取得する。存在しない場合は `Ok(None)`。
    ///
    /// # Errors
    ///
    /// 読み込みに失敗した場合、またはファイルが壊れている場合。
    /// **「存在しない」はエラーではない。** 地形は疎に焼かれるのが普通で、
    /// 海上のタイルは最初から作られない。
    fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError>;

    /// Whether this source may provide primary tiles, now or in the future.
    ///
    /// `false` promises that `load` always returns `Ok(None)` for this source's
    /// entire lifetime. Callers may skip primary reads and missing-tile history,
    /// but must still respect primary data already in their cache or meshes.
    /// Never infer this capability from a currently empty directory or cache:
    /// dynamic and third-party sources keep the conservative `true` default.
    fn primary_reads_possible(&self) -> bool {
        true
    }

    /// Optional immutable, in-memory render coverage hint. This never changes
    /// physical sampling, validates a payload, or rules out future primary reads.
    fn primary_coverage(&self) -> Option<&crate::PrimaryCoverage> {
        None
    }

    /// Whether a separate, complete-planet fallback pass is available.
    fn has_fallback(&self) -> bool {
        false
    }

    /// Generate one fallback tile only after the primary ancestor search has
    /// failed. Callers must count this against their normal per-frame budget.
    /// The default has no fallback; no runtime network access is implied.
    ///
    /// # Errors
    /// A source may report failure rather than silently fabricate terrain.
    fn load_fallback(&self, _id: TileId) -> Result<Option<DemTile>, TerrainError> {
        Ok(None)
    }

    /// Constant-work surface height after *all* primary levels have failed.
    /// Separate from tile generation so a generated fine tile cannot hide an
    /// existing real ancestor in the terrain cache.
    fn fallback_elevation_at(&self, _position: Geodetic) -> Option<Meters> {
        None
    }
}

/// タイル読み込みのエラー。
#[derive(Debug)]
pub enum TerrainError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// ファイルはあるが実行時タイルとして読めない。
    Malformed {
        path: PathBuf,
        source: TileReadError,
    },
    /// ファイル名から導いた ID と、中身に書かれた ID が食い違う。
    ///
    /// タイルを取り違えて配置すると、**全く違う場所の地形が静かに使われる**。
    IdMismatch {
        path: PathBuf,
        expected: TileId,
        found: TileId,
    },
}

impl core::fmt::Display for TerrainError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Malformed { path, source } => {
                write!(
                    formatter,
                    "{} is not a valid tile: {source}",
                    path.display()
                )
            }
            Self::IdMismatch {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "{} sits at the path for {expected:?} but declares {found:?}; \
                 using it would place the wrong terrain here",
                path.display()
            ),
        }
    }
}

impl std::error::Error for TerrainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { source, .. } => Some(source),
            Self::IdMismatch { .. } => None,
        }
    }
}

/// `{root}/{level}/{x}/{y}.fsdem` からタイルを読む。
///
/// `flightsim-tilegen` が書く配置と対になっている。
#[derive(Debug, Clone)]
pub struct DiskTileSource {
    root: PathBuf,
    coverage: Option<crate::PrimaryCoverage>,
}

impl DiskTileSource {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            coverage: None,
        }
    }

    /// Add a bounded metadata snapshot for render-only regional discovery.
    /// Call at source preparation, never during a streaming update. Recreate
    /// the snapshot to discover later additions; ordinary reads remain dynamic.
    ///
    /// # Errors
    /// Snapshot I/O/limits fail without returning a partial coverage index.
    pub fn with_coverage_index(mut self) -> Result<Self, TerrainError> {
        self.coverage = Some(crate::PrimaryCoverage::from_directory(&self.root).map_err(
            |source| TerrainError::Io {
                path: self.root.clone(),
                source,
            },
        )?);
        Ok(self)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl TileSource for DiskTileSource {
    fn primary_coverage(&self) -> Option<&crate::PrimaryCoverage> {
        self.coverage.as_ref()
    }
    fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        let path = self.root.join(tile_relative_path(id));

        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            // 焼かれていないタイルは存在しないのが正常。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(TerrainError::Io { path, source }),
        };

        use std::io::Read;
        let metadata = file.metadata().map_err(|source| TerrainError::Io {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file() || metadata.len() > MAX_TILE_FILE_BYTES {
            return Err(TerrainError::Io {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "tile exceeds encoded size limit or is not a regular file",
                ),
            });
        }
        // Stream the bounded input. Never allocate a Vec from an untrusted disk file's length.
        let mut reader = std::io::BufReader::new(file.take(MAX_TILE_FILE_BYTES + 1));
        let stored = read_tile(&mut reader).map_err(|source| TerrainError::Malformed {
            path: path.clone(),
            source,
        })?;

        if reader
            .read(&mut [0u8; 1])
            .map_err(|source| TerrainError::Io {
                path: path.clone(),
                source,
            })?
            != 0
        {
            return Err(TerrainError::Io {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "trailing bytes in tile",
                ),
            });
        }

        // パスと中身が食い違うタイルは使わない。使うと別の場所の地形になる。
        if stored.id != id {
            return Err(TerrainError::IdMismatch {
                path,
                expected: id,
                found: stored.id,
            });
        }

        Ok(Some(stored.tile))
    }
}

/// 箱に入った供給元も供給元として扱えるようにする。
///
/// 実行時に「ディスクから読む」「タイルを持たない」を切り替えたい呼び出し側が、
/// **存在しないパスを捏造する**という回避策に走らずに済むようにするため。
impl<T: TileSource + ?Sized> TileSource for Box<T> {
    fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        (**self).load(id)
    }
    fn primary_reads_possible(&self) -> bool {
        (**self).primary_reads_possible()
    }
    fn primary_coverage(&self) -> Option<&crate::PrimaryCoverage> {
        (**self).primary_coverage()
    }
    fn has_fallback(&self) -> bool {
        (**self).has_fallback()
    }
    fn load_fallback(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        (**self).load_fallback(id)
    }
    fn fallback_elevation_at(&self, position: Geodetic) -> Option<Meters> {
        (**self).fallback_elevation_at(position)
    }
}

/// 参照も供給元として扱えるようにする。
///
/// 1 つの供給元を複数の [`Terrain`] で共有したい場合（キャッシュ設定を変えて
/// 比べる、シナリオを並行して回す）に、タイルを丸ごと複製せずに済む。
impl<T: TileSource + ?Sized> TileSource for &T {
    fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        (**self).load(id)
    }
    fn primary_reads_possible(&self) -> bool {
        (**self).primary_reads_possible()
    }
    fn primary_coverage(&self) -> Option<&crate::PrimaryCoverage> {
        (**self).primary_coverage()
    }
    fn has_fallback(&self) -> bool {
        (**self).has_fallback()
    }
    fn load_fallback(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        (**self).load_fallback(id)
    }
    fn fallback_elevation_at(&self, position: Geodetic) -> Option<Meters> {
        (**self).fallback_elevation_at(position)
    }
}

/// An explicitly absent, immutable primary source.
///
/// Use only when no primary dataset is configured. An empty `MemoryTileSource`
/// or `DiskTileSource` may receive tiles later and must remain primary-capable.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyTileSource;

impl TileSource for EmptyTileSource {
    fn load(&self, _id: TileId) -> Result<Option<DemTile>, TerrainError> {
        Ok(None)
    }

    fn primary_reads_possible(&self) -> bool {
        false
    }
}

/// メモリ上のタイル集合。テストと、将来のアーカイブ読み込み用。
#[derive(Debug, Default)]
pub struct MemoryTileSource {
    tiles: std::collections::HashMap<TileId, DemTile>,
}

impl MemoryTileSource {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: TileId, tile: DemTile) {
        self.tiles.insert(id, tile);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

impl TileSource for MemoryTileSource {
    fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        Ok(self.tiles.get(&id).cloned())
    }
}

/// 見つからなかったタイルを覚えておく上限。
///
/// これが無いと、飛行するたびに存在しないタイルへのファイルアクセスが繰り返される。
/// 逆に無制限に覚えると、長距離飛行で集合が際限なく膨らむ。
const MISS_MEMORY_LIMIT: usize = 4_096;

/// Maximum retained primary-load diagnostics. A long global flight must not
/// accumulate an unbounded list of broken regional tiles. The first failures
/// remain inspectable; further failures are counted by `dropped_load_failures`.
pub const MAX_RECORDED_LOAD_FAILURES: usize = 4_096;

/// タイルをキャッシュしつつ測地座標の標高を返す。
///
/// # 決定論
///
/// キャッシュは**答えを変えない**。同じ座標に対しては、キャッシュの状態によらず
/// 同じ標高を返す。FDM の決定論（ADR-0004）を壊さないための前提。
#[derive(Debug)]
pub struct Terrain<S: TileSource> {
    source: S,
    cache: TileCache,
    /// 探索するレベル。深い方から順に試す。
    levels: core::ops::RangeInclusive<u8>,
    /// 存在しないと分かったタイル。ファイルアクセスの繰り返しを避ける。
    misses: HashSet<TileId>,
    load_failures: Vec<TerrainError>,
    dropped_load_failures: u64,
}

impl<S: TileSource> Terrain<S> {
    /// # Panics
    ///
    /// レベル範囲が逆転している場合、または [`MAX_LEVEL`] を超える場合。
    #[must_use]
    pub fn new(source: S, cache_bytes: usize, levels: core::ops::RangeInclusive<u8>) -> Self {
        assert!(
            levels.start() <= levels.end(),
            "level range {}..={} is inverted",
            levels.start(),
            levels.end()
        );
        assert!(
            *levels.end() <= MAX_LEVEL,
            "level {} exceeds MAX_LEVEL ({MAX_LEVEL})",
            levels.end()
        );
        Self {
            source,
            cache: TileCache::new(cache_bytes),
            levels,
            misses: HashSet::new(),
            load_failures: Vec::new(),
            dropped_load_failures: 0,
        }
    }

    /// 測地座標における地形標高（楕円体高）。
    ///
    /// 全 primary レベルを探し、無ければ source の global fallback を使う。
    /// どちらにもデータが無ければ `None`。呼び出し側が既定値を決める。
    /// ここで 0 m を返してしまうと、「本当に標高 0 m」と「データが無い」が
    /// 区別できなくなる。
    pub fn elevation_at(&mut self, position: Geodetic) -> Option<Meters> {
        let primary_reads_possible = self.source.primary_reads_possible();
        if !primary_reads_possible && self.cache.is_empty() {
            return self.source.fallback_elevation_at(position);
        }
        for level in self.levels.clone().rev() {
            let id = TileId::containing(level, position);

            if self.cache.contains(id) {
                return self.cache.get(id).map(|tile| tile.elevation_at(position));
            }
            if !primary_reads_possible || self.misses.contains(&id) {
                continue;
            }

            match self.source.load(id) {
                Ok(Some(tile)) => {
                    self.cache.insert(id, tile);
                    return self.cache.get(id).map(|tile| tile.elevation_at(position));
                }
                Ok(None) => self.remember_miss(id),
                Err(error) => {
                    // 壊れたタイル 1 枚で飛行全体を止めない。記録して次のレベルへ。
                    // 黙って無視すると「なぜ地形が平らなのか」が分からなくなる。
                    if self.load_failures.len() < MAX_RECORDED_LOAD_FAILURES {
                        self.load_failures.push(error);
                    } else {
                        self.dropped_load_failures = self.dropped_load_failures.saturating_add(1);
                    }
                    self.remember_miss(id);
                }
            }
        }
        self.source.fallback_elevation_at(position)
    }

    fn remember_miss(&mut self, id: TileId) {
        if self.misses.len() >= MISS_MEMORY_LIMIT {
            self.misses.clear();
        }
        self.misses.insert(id);
    }

    /// First primary tile load failures, bounded by `MAX_RECORDED_LOAD_FAILURES`.
    /// A global fallback may still cover these positions. Later failures are
    /// counted separately rather than growing memory during long flights.
    #[must_use]
    pub fn load_failures(&self) -> &[TerrainError] {
        &self.load_failures
    }

    /// Number of additional failures omitted after the diagnostic limit was
    /// reached. Saturates at u64::MAX; retained diagnostics are never cleared.
    #[must_use]
    pub const fn dropped_load_failures(&self) -> u64 {
        self.dropped_load_failures
    }

    #[must_use]
    pub const fn cache(&self) -> &TileCache {
        &self.cache
    }

    #[must_use]
    pub const fn source(&self) -> &S {
        &self.source
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "テスト用の標高データ生成。f32 の精度で十分"
    )]

    use super::*;
    use crate::dem::HeightGrid;
    use crate::dem::io::write_tile;

    /// 経度に比例して上る斜面のタイル。
    fn ramp_tile(id: TileId, size: u32, scale: f32) -> DemTile {
        let samples: Vec<f32> = (0..size)
            .flat_map(|_| (0..size).map(move |column| column as f32 * scale))
            .collect();
        DemTile::new(id.bounds(), HeightGrid::new(size, size, samples))
    }

    fn memory_terrain(tiles: Vec<(TileId, DemTile)>) -> Terrain<MemoryTileSource> {
        let mut source = MemoryTileSource::new();
        for (id, tile) in tiles {
            source.insert(id, tile);
        }
        Terrain::new(source, 16 * 1024 * 1024, 0..=12)
    }

    #[test]
    fn only_explicit_empty_sources_opt_out_of_primary_discovery() {
        let id = TileId::roots()[0];
        assert!(!EmptyTileSource.primary_reads_possible());
        assert!(EmptyTileSource.load(id).unwrap().is_none());
        let mut memory = MemoryTileSource::new();
        assert!(memory.primary_reads_possible());
        memory.insert(id, ramp_tile(id, 3, 10.0));
        assert!(memory.primary_reads_possible());
        assert!(memory.load(id).unwrap().is_some());
        assert!(
            DiskTileSource::new("unconfigured-but-possibly-future-directory")
                .primary_reads_possible()
        );
        let boxed: Box<dyn TileSource> = Box::new(EmptyTileSource);
        assert!(!boxed.primary_reads_possible());
        fn reference_primary_reads_possible<S: TileSource>(source: S) -> bool {
            source.primary_reads_possible()
        }
        assert!(!reference_primary_reads_possible(&boxed));
        assert!(reference_primary_reads_possible(&memory));
    }

    #[test]
    fn an_explicit_absent_primary_skips_reads_and_history_but_preserves_cached_primary() {
        #[derive(Debug)]
        struct NoPrimary;
        impl TileSource for NoPrimary {
            fn load(&self, _id: TileId) -> Result<Option<DemTile>, TerrainError> {
                panic!("an explicitly absent primary must not be probed");
            }
            fn primary_reads_possible(&self) -> bool {
                false
            }
            fn fallback_elevation_at(&self, _position: Geodetic) -> Option<Meters> {
                Some(Meters(5.0))
            }
        }
        let coarse = TileId::new(3, 5, 2);
        let fine = coarse.children().unwrap()[0];
        let position = fine.center();
        let mut terrain = Terrain::new(NoPrimary, 1_000_000, 0..=13);
        assert_eq!(terrain.elevation_at(position), Some(Meters(5.0)));
        terrain.cache.insert(
            coarse,
            DemTile::new(coarse.bounds(), HeightGrid::flat(3, 3, Meters(42.0))),
        );
        assert_eq!(terrain.elevation_at(position), Some(Meters(42.0)));
        terrain.cache.insert(
            fine,
            DemTile::new(fine.bounds(), HeightGrid::flat(3, 3, Meters(78.0))),
        );
        assert_eq!(terrain.elevation_at(position), Some(Meters(78.0)));
        assert!(terrain.misses.is_empty());
        assert!(terrain.load_failures().is_empty());
    }

    // --- 基本 ---

    #[test]
    fn elevation_comes_from_the_tile_that_contains_the_position() {
        let id = TileId::new(10, 500, 300);
        let mut terrain = memory_terrain(vec![(id, ramp_tile(id, 17, 100.0))]);

        let centre = id.center();
        let elevation = terrain.elevation_at(centre).expect("the tile exists");
        // 17 点の斜面の中央 = 8 * 100 = 800 m
        assert!(
            (elevation.get() - 800.0).abs() < 1.0,
            "sampled {elevation} m at the tile centre, expected about 800 m"
        );
    }

    #[test]
    fn a_position_with_no_tile_reports_no_data_rather_than_zero() {
        // 0 m を返すと「本当に海面」と「データが無い」が区別できなくなる。
        let mut terrain = memory_terrain(vec![]);
        assert!(
            terrain
                .elevation_at(Geodetic::from_degrees(35.0, 139.0, 0.0))
                .is_none()
        );
    }

    #[test]
    fn the_deepest_available_level_wins() {
        let coarse = TileId::new(8, 125, 75);
        let fine = TileId::containing(11, coarse.center());

        let mut terrain = memory_terrain(vec![
            (
                coarse,
                DemTile::new(coarse.bounds(), HeightGrid::flat(9, 9, Meters(100.0))),
            ),
            (
                fine,
                DemTile::new(fine.bounds(), HeightGrid::flat(9, 9, Meters(900.0))),
            ),
        ]);

        let elevation = terrain
            .elevation_at(fine.center())
            .expect("both levels cover this point");
        assert!(
            (elevation.get() - 900.0).abs() < 1e-6,
            "expected the level 11 tile (900 m), got {elevation}"
        );
    }

    #[test]
    fn a_missing_fine_tile_falls_back_to_a_coarser_one() {
        let coarse = TileId::new(8, 125, 75);
        let mut terrain = memory_terrain(vec![(
            coarse,
            DemTile::new(coarse.bounds(), HeightGrid::flat(9, 9, Meters(250.0))),
        )]);

        let elevation = terrain
            .elevation_at(coarse.center())
            .expect("the coarse tile covers this point");
        assert!((elevation.get() - 250.0).abs() < 1e-6);
    }

    // --- 連続性 ---

    #[test]
    fn elevation_is_continuous_across_a_tile_boundary() {
        // 隣接タイルは境界上の格子点を共有する。ここが不連続だと、
        // タイルをまたぐたびに機体が段差を踏む。
        let west = TileId::new(10, 500, 300);
        let east = west
            .neighbour(crate::tile::Direction::East)
            .expect("has an eastern neighbour");

        // 両タイルの境界（west の東端 = east の西端）で同じ標高になるデータを作る。
        let boundary_elevation = 1_234.0_f32;
        let mut west_samples = vec![0.0_f32; 9 * 9];
        let mut east_samples = vec![0.0_f32; 9 * 9];
        for row in 0..9 {
            for column in 0..9 {
                // west は東端で、east は西端で boundary_elevation になる線形斜面。
                west_samples[row * 9 + column] = boundary_elevation * (column as f32) / 8.0;
                east_samples[row * 9 + column] = boundary_elevation * (1.0 - (column as f32) / 8.0);
            }
        }

        let mut terrain = memory_terrain(vec![
            (
                west,
                DemTile::new(west.bounds(), HeightGrid::new(9, 9, west_samples)),
            ),
            (
                east,
                DemTile::new(east.bounds(), HeightGrid::new(9, 9, east_samples)),
            ),
        ]);

        let boundary_longitude = west.bounds().east.get();
        let latitude = west.center().latitude;
        let step = west.bounds().width().get() * 1e-6;

        let just_west = terrain
            .elevation_at(Geodetic::new(
                latitude,
                flightsim_core::Radians(boundary_longitude - step),
                Meters::ZERO,
            ))
            .expect("west tile");
        let just_east = terrain
            .elevation_at(Geodetic::new(
                latitude,
                flightsim_core::Radians(boundary_longitude + step),
                Meters::ZERO,
            ))
            .expect("east tile");

        assert!(
            (just_west.get() - just_east.get()).abs() < 0.1,
            "elevation jumped from {just_west} to {just_east} across the tile boundary"
        );
    }

    // --- キャッシュ ---

    #[test]
    fn caching_does_not_change_the_answer() {
        // キャッシュが答えを変えると FDM の決定論が壊れる。
        let id = TileId::new(10, 500, 300);
        let tile = ramp_tile(id, 17, 100.0);
        let mut terrain = memory_terrain(vec![(id, tile)]);

        let probe = id.center();
        let first = terrain.elevation_at(probe).expect("tile exists");
        for _ in 0..50 {
            let repeated = terrain.elevation_at(probe).expect("tile exists");
            assert!(
                (first.get() - repeated.get()).abs() < 1e-12,
                "cached lookup returned {repeated} where the first returned {first}"
            );
        }
        assert!(!terrain.cache().is_empty(), "nothing was cached at all");
    }

    #[test]
    fn missing_tiles_are_only_looked_up_once() {
        /// 読み込み回数を数える供給元。
        #[derive(Debug, Default)]
        struct CountingSource {
            attempts: std::cell::Cell<usize>,
        }
        impl TileSource for CountingSource {
            fn load(&self, _id: TileId) -> Result<Option<DemTile>, TerrainError> {
                self.attempts.set(self.attempts.get() + 1);
                Ok(None)
            }
        }

        let mut terrain = Terrain::new(CountingSource::default(), 1024 * 1024, 10..=10);
        let probe = Geodetic::from_degrees(35.0, 139.0, 0.0);

        for _ in 0..20 {
            assert!(terrain.elevation_at(probe).is_none());
        }
        assert_eq!(
            terrain.source().attempts.get(),
            1,
            "a missing tile was looked up more than once"
        );
    }

    #[test]
    fn long_global_travel_keeps_failure_diagnostics_bounded() {
        #[derive(Debug)]
        struct BrokenSource;
        impl TileSource for BrokenSource {
            fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
                Err(TerrainError::Io {
                    path: PathBuf::from(format!("broken-{}-{}", id.x, id.y)),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, "fixture failure"),
                })
            }
            fn fallback_elevation_at(&self, _position: Geodetic) -> Option<Meters> {
                Some(Meters(25.0))
            }
        }
        let mut terrain = Terrain::new(BrokenSource, 1_024, 9..=9);
        for index in 0..MAX_RECORDED_LOAD_FAILURES + 17 {
            let index = u32::try_from(index).unwrap();
            let id = TileId::new(9, index % 1_024, index / 1_024);
            assert_eq!(terrain.elevation_at(id.center()), Some(Meters(25.0)));
            assert!(terrain.load_failures().len() <= MAX_RECORDED_LOAD_FAILURES);
            assert!(terrain.misses.len() <= MISS_MEMORY_LIMIT);
        }
        assert_eq!(terrain.load_failures().len(), MAX_RECORDED_LOAD_FAILURES);
        assert_eq!(terrain.dropped_load_failures(), 17);
        assert!(terrain.cache().is_empty());
    }

    // --- ディスク ---

    #[test]
    fn tiles_written_to_disk_are_read_back_through_the_source() {
        let directory = std::env::temp_dir().join(format!(
            "flightsim-terrain-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("temp dir");

        let id = TileId::new(9, 300, 180);
        let tile = ramp_tile(id, 17, 50.0);
        let path = directory.join(tile_relative_path(id));
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("tile dir");
        let mut bytes = Vec::new();
        write_tile(&mut bytes, id, tile.grid()).expect("write");
        std::fs::write(&path, bytes).expect("write tile");

        let mut terrain = Terrain::new(DiskTileSource::new(&directory), 1024 * 1024, 9..=9);
        let elevation = terrain
            .elevation_at(id.center())
            .expect("the tile is on disk");
        assert!(
            (elevation.get() - 400.0).abs() < 1.0,
            "sampled {elevation} m"
        );
        assert!(terrain.load_failures().is_empty());

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn oversized_disk_tiles_and_trailing_data_are_rejected_before_unbounded_reads() {
        let directory = std::env::temp_dir().join(format!(
            "flightsim-terrain-bounded-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let id = TileId::new(9, 300, 180);
        let path = directory.join(tile_relative_path(id));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let oversized = std::fs::File::create(&path).unwrap();
        oversized.set_len(MAX_TILE_FILE_BYTES + 1).unwrap();
        drop(oversized);
        let source = DiskTileSource::new(&directory);
        assert!(matches!(source.load(id), Err(TerrainError::Io { .. })));
        let mut bytes = Vec::new();
        write_tile(&mut bytes, id, ramp_tile(id, 9, 10.0).grid()).unwrap();
        bytes.push(0);
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(source.load(id), Err(TerrainError::Io { .. })));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_tile_at_the_wrong_path_is_rejected_rather_than_used() {
        // 取り違えて配置されたタイルを使うと、全く違う場所の地形が静かに使われる。
        let directory = std::env::temp_dir().join(format!(
            "flightsim-terrain-mismatch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let wanted = TileId::new(9, 300, 180);
        let actual = TileId::new(9, 301, 180);

        let path = directory.join(tile_relative_path(wanted));
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("tile dir");
        let mut bytes = Vec::new();
        write_tile(&mut bytes, actual, ramp_tile(actual, 9, 10.0).grid()).expect("write");
        std::fs::write(&path, bytes).expect("write tile");

        let source = DiskTileSource::new(&directory);
        assert!(matches!(
            source.load(wanted),
            Err(TerrainError::IdMismatch { .. })
        ));

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_corrupt_tile_is_recorded_without_stopping_the_lookup() {
        let directory = std::env::temp_dir().join(format!(
            "flightsim-terrain-corrupt-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let id = TileId::new(9, 300, 180);
        let path = directory.join(tile_relative_path(id));
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("tile dir");
        std::fs::write(&path, b"this is not a tile file at all").expect("write garbage");

        let mut terrain = Terrain::new(DiskTileSource::new(&directory), 1024 * 1024, 9..=9);
        assert!(terrain.elevation_at(id.center()).is_none());
        assert_eq!(
            terrain.load_failures().len(),
            1,
            "a corrupt tile should be reported, not silently ignored"
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_missing_directory_reports_no_data_rather_than_failing() {
        // 海上を飛ぶときにタイルが無いのは正常。エラーにしない。
        let mut terrain = Terrain::new(
            DiskTileSource::new("this-directory-does-not-exist"),
            1024 * 1024,
            9..=9,
        );
        assert!(
            terrain
                .elevation_at(Geodetic::from_degrees(0.0, 0.0, 0.0))
                .is_none()
        );
        assert!(terrain.load_failures().is_empty());
    }

    #[test]
    #[should_panic(expected = "is inverted")]
    fn an_inverted_level_range_is_rejected() {
        // リテラルの `12..=8` は clippy が空レンジとして弾く。
        // ここでは逆転レンジを渡した呼び出し側を再現したいので明示的に作る。
        let inverted = core::ops::RangeInclusive::new(12_u8, 8_u8);
        let _ = Terrain::new(MemoryTileSource::new(), 1024, inverted);
    }
}
