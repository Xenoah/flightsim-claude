//! # flightsim-tilegen
//!
//! Copernicus DEM の GeoTIFF から、実行時タイル `.fsdem` を焼くオフライン CLI。
//!
//! ## 位置づけ
//!
//! **実行時には一切動かない。** 実行時に GeoTIFF をパースするとフレーム予算に
//! 収まらないため、オフラインで中間形式へ焼く（[ADR-0003]）。その中間形式の
//! 仕様は [ADR-0005] にある。
//!
//! ```text
//!   Copernicus DEM (GeoTIFF)  ──[このツール]──>  tiles/{level}/{x}/{y}.fsdem
//!                                                        │
//!                                                        └─> flightsim-world が実行時に読む
//! ```
//!
//! 依存の向きは `core` ← `world` ← `tilegen`。上位のツールなので `flightsim-world`
//! に依存してよい。ただし `bevy` は不可（焼き込みに描画エンジンは要らない）。
//!
//! ## 使い方
//!
//! ```bash
//! flightsim-tilegen \
//!     --input Copernicus_DSM_COG_10_N35_00_E139_00_DEM.tif \
//!     --output data/tiles \
//!     --min-level 8 --max-level 12
//! ```
//!
//! 範囲を絞る場合は `--bounds west,south,east,north`（度）。省略すると入力ラスタの
//! 被覆範囲を使う。**全球を一度に焼こうとしないこと** — level 12 だけで
//! 3 350 万タイルになる。
//!
//! ## 構成
//!
//! | モジュール | 役割 |
//! |---|---|
//! | [`geotiff`] | GeoTIFF の読み込みと地理参照。EPSG:4326 の単バンド浮動小数点のみ |
//! | [`region`] | 焼き込み範囲とタイル列挙。日付変更線・極を扱う |
//! | [`generate`] | ラスタからタイルを焼き、`.fsdem` として書き出す |
//! | [`testing`] | 合成 GeoTIFF の組み立て。CI が実データを必要としないため |
//!
//! [ADR-0003]: https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0003-terrain-data.md
//! [ADR-0005]: https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0005-runtime-tile-format.md

pub mod generate;
pub mod geotiff;
pub mod region;
pub mod testing;

pub use generate::{
    GenerateError, GenerationReport, RasterSet, TileBuild, TileGenOptions, build_tile,
    generate_tiles,
};
pub use geotiff::{GeoRaster, RasterCoverage, RasterError};
pub use region::{Region, RegionError};
