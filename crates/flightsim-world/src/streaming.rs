//! タイルの読み込みスケジューリングとキャッシュ。
//!
//! # 1 フレームの処理量に必ず上限を設ける
//!
//! 上限が無いと、高速で飛行した際に大量のタイル読み込みが同一フレームに集中し、
//! 目に見えるスタッターになる。**フレーム予算はこのモジュールの中心的な要件であり、
//! 「速いマシンなら大丈夫」で省略してよいものではない。**
//!
//! # 優先度
//!
//! カメラからの距離が近いタイルを先に読む。距離は `f64` のビット表現を鍵として
//! 使う（非負の有限 `f64` ではビット列が値の大小と同じ順序になるため、
//! 精度を落とさずに整数比較へ落とせる）。

use crate::dem::DemTile;
use crate::tile::TileId;
use flightsim_core::Meters;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// 1 フレームで取り出すタイル数の既定上限。
pub const DEFAULT_MAX_LOADS_PER_FRAME: usize = 8;

/// 距離を順序比較可能な整数鍵へ変換する。
///
/// 非負の有限 `f64` は、IEEE 754 のビット列が値と同じ順序になる。
/// `NaN` は最大値（＝最低優先度）に落とす。
fn priority_key(distance: Meters) -> u64 {
    let value = distance.get();
    if value.is_nan() {
        u64::MAX
    } else {
        value.max(0.0).to_bits()
    }
}

/// 読み込み待ちタイルの優先度キュー。
///
/// 同じタイルがより近い距離で再要求された場合、優先度を引き上げる。
/// `BinaryHeap` は優先度の更新に対応しないため、古い要素は取り出し時に読み飛ばす
/// （遅延削除）。
#[derive(Debug, Default)]
pub struct StreamingScheduler {
    queue: BinaryHeap<Reverse<(u64, TileId)>>,
    /// 各タイルの現時点で最良の優先度鍵。遅延削除の判定に使う。
    best: HashMap<TileId, u64>,
    max_loads_per_frame: usize,
}

impl StreamingScheduler {
    #[must_use]
    pub fn new(max_loads_per_frame: usize) -> Self {
        assert!(
            max_loads_per_frame > 0,
            "the per-frame load budget must be positive; \
             zero would stall streaming entirely"
        );
        Self {
            queue: BinaryHeap::new(),
            best: HashMap::new(),
            max_loads_per_frame,
        }
    }

    /// タイルの読み込みを要求する。既に待機中なら、より近い距離のときだけ優先度を上げる。
    pub fn request(&mut self, tile: TileId, distance: Meters) {
        let key = priority_key(distance);

        match self.best.get(&tile) {
            Some(&existing) if existing <= key => {}
            _ => {
                self.best.insert(tile, key);
                self.queue.push(Reverse((key, tile)));
            }
        }
    }

    /// 待機中のタイル数。
    #[must_use]
    pub fn pending(&self) -> usize {
        self.best.len()
    }

    #[must_use]
    pub const fn max_loads_per_frame(&self) -> usize {
        self.max_loads_per_frame
    }

    /// このフレームで読み込むタイルを、近い順に予算ぶんだけ取り出す。
    ///
    /// **戻り値の長さは必ず `max_loads_per_frame` 以下。** これがフレームスパイクを防ぐ。
    pub fn take_batch(&mut self) -> Vec<TileId> {
        let mut batch = Vec::with_capacity(self.max_loads_per_frame);

        while batch.len() < self.max_loads_per_frame {
            let Some(Reverse((key, tile))) = self.queue.pop() else {
                break;
            };

            // 遅延削除。優先度が更新された古い要素は読み飛ばす。
            match self.best.get(&tile) {
                Some(&best) if best == key => {
                    self.best.remove(&tile);
                    batch.push(tile);
                }
                _ => {}
            }
        }

        batch
    }

    /// 待機列を空にする。カメラのテレポートやシナリオ切り替えで使う。
    pub fn clear(&mut self) {
        self.queue.clear();
        self.best.clear();
    }
}

#[derive(Debug)]
struct CacheEntry {
    tile: DemTile,
    footprint: usize,
    last_used: u64,
}

/// バイト数上限を持つ LRU タイルキャッシュ。
///
/// **上限を持たないキャッシュを作らないこと。** 全球を飛ぶと、上限が無ければ
/// メモリを際限なく消費する。
///
/// # 計算量
///
/// 追い出しは最終使用時刻の線形走査（O(n)）。想定するタイル数は数百程度なので
/// これで十分だが、数万規模になったら順序付きの索引を持つこと。
#[derive(Debug)]
pub struct TileCache {
    capacity_bytes: usize,
    used_bytes: usize,
    entries: HashMap<TileId, CacheEntry>,
    clock: u64,
}

impl TileCache {
    /// # Panics
    ///
    /// 容量がゼロの場合にパニックする。何も保持できないキャッシュは、
    /// 毎フレーム全タイルを読み直すという最悪の挙動になる。
    #[must_use]
    pub fn new(capacity_bytes: usize) -> Self {
        assert!(capacity_bytes > 0, "cache capacity must be positive");
        Self {
            capacity_bytes,
            used_bytes: 0,
            entries: HashMap::new(),
            clock: 0,
        }
    }

    #[must_use]
    pub const fn capacity_bytes(&self) -> usize {
        self.capacity_bytes
    }

    #[must_use]
    pub const fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn contains(&self, tile: TileId) -> bool {
        self.entries.contains_key(&tile)
    }

    /// タイルを取得し、最終使用時刻を更新する。
    pub fn get(&mut self, tile: TileId) -> Option<&DemTile> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self.entries.get_mut(&tile)?;
        entry.last_used = clock;
        Some(&entry.tile)
    }

    /// 最終使用時刻を更新せずに覗く。デバッグ表示や統計に使う。
    #[must_use]
    pub fn peek(&self, tile: TileId) -> Option<&DemTile> {
        self.entries.get(&tile).map(|entry| &entry.tile)
    }

    /// タイルを格納する。容量を超える場合は古いものから追い出す。
    ///
    /// 1 枚で容量を超えるタイルは、キャッシュを空にしたうえで格納する。
    /// この場合だけ [`Self::used_bytes`] が容量を上回る。
    /// 拒否すると地形が永久に読み込まれなくなるため、格納を優先している。
    pub fn insert(&mut self, tile: TileId, dem: DemTile) {
        self.clock += 1;
        let footprint = dem.memory_footprint();

        if let Some(previous) = self.entries.remove(&tile) {
            self.used_bytes -= previous.footprint;
        }

        self.evict_until_fits(footprint);

        self.used_bytes += footprint;
        self.entries.insert(
            tile,
            CacheEntry {
                tile: dem,
                footprint,
                last_used: self.clock,
            },
        );
    }

    /// 追加分が収まるまで、最も長く使われていないタイルを追い出す。
    fn evict_until_fits(&mut self, incoming: usize) {
        while !self.entries.is_empty() && self.used_bytes + incoming > self.capacity_bytes {
            let Some(&victim) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(id, _)| id)
            else {
                break;
            };

            if let Some(entry) = self.entries.remove(&victim) {
                self.used_bytes -= entry.footprint;
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dem::HeightGrid;

    fn dem(size: u32) -> DemTile {
        DemTile::new(
            TileId::new(3, 0, 0).bounds(),
            HeightGrid::flat(size, size, Meters(100.0)),
        )
    }

    /// テスト用に区別できるタイルを作る。level 9 は 1024 列あるので、
    /// 数百枚を扱うテストでも添字が範囲を外れない。
    fn tile(index: u32) -> TileId {
        TileId::new(9, index, 0)
    }

    // --- スケジューラ ---

    #[test]
    fn nearest_tiles_are_loaded_first() {
        let mut scheduler = StreamingScheduler::new(3);
        scheduler.request(tile(0), Meters(50_000.0));
        scheduler.request(tile(1), Meters(1_000.0));
        scheduler.request(tile(2), Meters(10_000.0));
        scheduler.request(tile(3), Meters(200.0));

        let batch = scheduler.take_batch();
        assert_eq!(batch, vec![tile(3), tile(1), tile(2)]);
    }

    #[test]
    fn the_per_frame_budget_is_never_exceeded() {
        // ストリーミングの中心的な要件。これが破れるとフレームスパイクになる。
        let mut scheduler = StreamingScheduler::new(4);
        for i in 0..500 {
            scheduler.request(tile(i), Meters(f64::from(i)));
        }

        for _ in 0..10 {
            assert!(scheduler.take_batch().len() <= 4);
        }
    }

    #[test]
    fn every_requested_tile_is_eventually_delivered() {
        let mut scheduler = StreamingScheduler::new(7);
        for i in 0..100 {
            scheduler.request(tile(i), Meters(f64::from(100 - i)));
        }

        let mut delivered = Vec::new();
        while scheduler.pending() > 0 {
            let batch = scheduler.take_batch();
            assert!(
                !batch.is_empty(),
                "the scheduler stalled with work still pending"
            );
            delivered.extend(batch);
        }

        assert_eq!(delivered.len(), 100);
        delivered.sort_unstable();
        delivered.dedup();
        assert_eq!(delivered.len(), 100, "a tile was delivered more than once");
    }

    #[test]
    fn requesting_the_same_tile_twice_does_not_duplicate_work() {
        let mut scheduler = StreamingScheduler::new(8);
        for _ in 0..20 {
            scheduler.request(tile(0), Meters(5_000.0));
        }
        assert_eq!(scheduler.pending(), 1);
        assert_eq!(scheduler.take_batch(), vec![tile(0)]);
        assert_eq!(scheduler.pending(), 0);
    }

    #[test]
    fn approaching_a_tile_raises_its_priority() {
        // カメラが近づいたタイルは、先に要求された遠いタイルより優先される。
        let mut scheduler = StreamingScheduler::new(2);
        scheduler.request(tile(0), Meters(90_000.0));
        scheduler.request(tile(1), Meters(20_000.0));
        // カメラが tile(0) へ接近した。
        scheduler.request(tile(0), Meters(500.0));

        assert_eq!(scheduler.take_batch(), vec![tile(0), tile(1)]);
    }

    #[test]
    fn receding_from_a_tile_does_not_lower_its_priority() {
        // 一度上げた優先度を下げると、遠ざかったり近づいたりを繰り返すカメラで
        // タイルが永久に読み込まれなくなる。
        let mut scheduler = StreamingScheduler::new(2);
        scheduler.request(tile(0), Meters(100.0));
        scheduler.request(tile(0), Meters(80_000.0));
        scheduler.request(tile(1), Meters(1_000.0));

        assert_eq!(scheduler.take_batch(), vec![tile(0), tile(1)]);
    }

    #[test]
    fn nan_distances_sort_last_without_breaking_the_queue() {
        let mut scheduler = StreamingScheduler::new(3);
        scheduler.request(tile(0), Meters(f64::NAN));
        scheduler.request(tile(1), Meters(5_000.0));
        scheduler.request(tile(2), Meters(100.0));

        assert_eq!(scheduler.take_batch(), vec![tile(2), tile(1), tile(0)]);
    }

    #[test]
    fn clearing_discards_pending_work() {
        let mut scheduler = StreamingScheduler::new(4);
        for i in 0..10 {
            scheduler.request(tile(i), Meters(f64::from(i)));
        }
        scheduler.clear();
        assert_eq!(scheduler.pending(), 0);
        assert!(scheduler.take_batch().is_empty());
    }

    // --- キャッシュ ---

    #[test]
    fn stored_tiles_can_be_retrieved() {
        let mut cache = TileCache::new(10_000_000);
        cache.insert(tile(0), dem(16));

        assert!(cache.contains(tile(0)));
        assert!(cache.get(tile(0)).is_some());
        assert!(cache.get(tile(1)).is_none());
    }

    #[test]
    fn the_cache_never_exceeds_its_byte_budget() {
        // 上限のないキャッシュは全球を飛ぶとメモリを食い尽くす。
        let one_tile = dem(32).memory_footprint();
        let capacity = one_tile * 5;
        let mut cache = TileCache::new(capacity);

        for i in 0..100 {
            cache.insert(tile(i), dem(32));
            assert!(
                cache.used_bytes() <= capacity,
                "cache grew to {} bytes against a {capacity} byte budget",
                cache.used_bytes()
            );
        }
        assert_eq!(cache.len(), 5);
    }

    #[test]
    fn least_recently_used_tiles_are_evicted_first() {
        let one_tile = dem(16).memory_footprint();
        let mut cache = TileCache::new(one_tile * 3);

        cache.insert(tile(0), dem(16));
        cache.insert(tile(1), dem(16));
        cache.insert(tile(2), dem(16));

        // tile(0) と tile(2) を触って新しくする。tile(1) が最も古くなる。
        assert!(cache.get(tile(0)).is_some());
        assert!(cache.get(tile(2)).is_some());

        cache.insert(tile(3), dem(16));

        assert!(cache.contains(tile(0)), "a recently used tile was evicted");
        assert!(cache.contains(tile(2)), "a recently used tile was evicted");
        assert!(cache.contains(tile(3)));
        assert!(
            !cache.contains(tile(1)),
            "the least recently used tile survived"
        );
    }

    #[test]
    fn peeking_does_not_affect_eviction_order() {
        let one_tile = dem(16).memory_footprint();
        let mut cache = TileCache::new(one_tile * 2);

        cache.insert(tile(0), dem(16));
        cache.insert(tile(1), dem(16));

        // peek は最終使用時刻を更新しない。tile(0) は依然として最も古い。
        assert!(cache.peek(tile(0)).is_some());
        cache.insert(tile(2), dem(16));

        assert!(
            !cache.contains(tile(0)),
            "peek should not have refreshed the entry"
        );
    }

    #[test]
    fn reinserting_a_tile_does_not_double_count_its_size() {
        let mut cache = TileCache::new(10_000_000);
        cache.insert(tile(0), dem(32));
        let after_first = cache.used_bytes();

        cache.insert(tile(0), dem(32));
        assert_eq!(cache.used_bytes(), after_first);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn an_oversized_tile_is_still_stored() {
        // 拒否すると、その地形は永久に読み込まれない。
        // 容量を一時的に超えることを許容し、その事実を明示する。
        let big = dem(256);
        let mut cache = TileCache::new(big.memory_footprint() / 4);
        cache.insert(tile(0), big);

        assert!(cache.contains(tile(0)));
        assert_eq!(
            cache.len(),
            1,
            "the oversized tile should be alone in the cache"
        );
    }

    #[test]
    fn clearing_releases_all_accounted_bytes() {
        let mut cache = TileCache::new(10_000_000);
        for i in 0..10 {
            cache.insert(tile(i), dem(16));
        }
        cache.clear();

        assert!(cache.is_empty());
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    #[should_panic(expected = "per-frame load budget must be positive")]
    fn a_zero_frame_budget_is_rejected() {
        let _ = StreamingScheduler::new(0);
    }

    #[test]
    #[should_panic(expected = "cache capacity must be positive")]
    fn a_zero_capacity_cache_is_rejected() {
        let _ = TileCache::new(0);
    }
}
