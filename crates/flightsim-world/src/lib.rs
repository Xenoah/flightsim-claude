//! # flightsim-world
//!
//! 地形タイル・DEM・LOD 選択・ストリーミング。
//!
//! ## 設計上の制約
//!
//! - **Bevy に依存しない。** メッシュの「データ」までが担当で、GPU バッファへの投入は
//!   `flightsim-render` の責務（ADR-0001）。
//! - **実行時に生データ（GeoTIFF / OSM PBF）をパースしない。** オフラインで中間形式へ
//!   焼き、実行時はそれを読むだけにする。フレーム予算に収まらない（ADR-0003）。
//! - **タイルスキームは Cesium geographic tiling scheme と互換。** 将来の商用タイル
//!   併用の余地を残すための意図的な決定。
//!
//! ## 構成
//!
//! | モジュール | 役割 |
//! |---|---|
//! | [`airport`] | 飛行場の幾何。滑走路の位置・向き・矩形上の判定 |
//! | [`tile`] | 地理座標系クアッドツリー。タイル ID と範囲、隣接関係 |
//! | [`dem`] | 標高格子とバイリニアサンプリング、幾何誤差の算出 |
//! | [`dem::io`] | 実行時タイル形式 `.fsdem` の読み書き（ADR-0005） |
//! | [`lod`] | 幾何誤差ベースの screen-space error による細分化判定 |
//! | [`streaming`] | 優先度付き読み込みキューと、バイト数上限つき LRU キャッシュ |
//! | [`terrain`] | 焼かれたタイルから測地座標の標高を引く層 |
//! | [`mesh`] | 描画用メッシュ**データ**の生成。GPU には触らない |
//!
//! ## 使い方
//!
//! ```
//! use flightsim_core::{Degrees, Geodetic, Meters};
//! use flightsim_world::{LodSelector, StreamingScheduler, TileCache, lod::distance_to_bounds};
//!
//! let selector = LodSelector::new(
//!     16.0,                       // 許容 screen-space error `px`
//!     1_080.0,                    // ビューポート高さ `px`
//!     Degrees(60.0).to_radians(), // 垂直画角
//!     12,                         // 最大レベル
//!     Meters(20_000.0),           // level 0 の幾何誤差
//! );
//!
//! let camera = Geodetic::from_degrees(35.55, 139.78, 3_000.0).to_ecef();
//! let selection = selector.select(camera);
//! assert!(!selection.truncated);
//!
//! // 選ばれたタイルのうち未キャッシュのものを、距離順に読み込み要求する。
//! let mut cache = TileCache::new(256 * 1024 * 1024);
//! let mut scheduler = StreamingScheduler::new(8);
//!
//! for tile in &selection.tiles {
//!     if !cache.contains(*tile) {
//!         let distance = distance_to_bounds(camera, camera.to_geodetic(), tile.bounds());
//!         scheduler.request(*tile, distance);
//!     }
//! }
//!
//! // 1 フレームで読むのは予算ぶんだけ。ここを無制限にするとスタッターになる。
//! let to_load = scheduler.take_batch();
//! assert!(to_load.len() <= scheduler.max_loads_per_frame());
//! ```

pub mod airport;
pub mod dem;
pub mod lod;
pub mod mesh;
pub mod streaming;
pub mod terrain;
pub mod tile;

pub use airport::io::{
    AirportDatabase, AirportDatabaseError, AirportRunway, AirportTaxiway, TaxiwayGeometryError,
};
pub use airport::{Runway, RunwayGeometryError, RunwayOffsets};
pub use dem::io::{StoredTile, TileReadError, TileWriteError, read_tile, write_tile};
pub use dem::{DemTile, HeightGrid};
pub use lod::{LodSelection, LodSelector};
pub use mesh::{MeshOptions, TerrainMesh, build_mesh};
pub use streaming::{StreamingScheduler, TileCache};
pub use terrain::{DiskTileSource, MemoryTileSource, Terrain, TerrainError, TileSource};
pub use tile::{Direction, GeoBounds, TileId};
