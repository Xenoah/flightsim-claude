//! # flightsim-core
//!
//! 座標系・単位・時間刻みの共通基盤。**このクレートは他のいかなるクレートにも依存しない。**
//!
//! ## このクレートが存在する理由
//!
//! 座標変換と単位変換を各モジュールが独自に実装すると、丸め規約と特異点（極・日付変更線）の
//! 扱いが分岐し、原因特定が極めて困難なズレを生む。変換の入口をここ一箇所に集約するのが目的。
//!
//! **他クレートで `sin`/`cos` を使った測地変換を書くことは禁止されている**（[ADR-0002]）。
//!
//! ## 座標系の階層
//!
//! | 系 | 型 | 用途 |
//! |---|---|---|
//! | Geodetic | [`Geodetic`] | 入出力・タイル索引・空港位置 |
//! | ECEF | [`Ecef`] | **世界の正準座標。**物理積分はここ（`f64`） |
//! | NED | [`Ned`] | 姿勢・風・航法計器（ローカル接平面） |
//! | Render | `Vec3` | [`FloatingOrigin`] 適用後の `f32`。描画専用 |
//!
//! [ADR-0002]: https://github.com/../docs/adr/0002-coordinate-system.md

pub mod fixed_step;
pub mod frames;
pub mod geodetic;
pub mod origin;
pub mod render_frame;
pub mod units;

pub use fixed_step::FixedStep;
pub use frames::{Attitude, LocalFrame, Ned};
pub use geodetic::{Ecef, Geodetic, wgs84};
pub use origin::FloatingOrigin;
pub use render_frame::RenderFrame;
pub use units::{
    Degrees, Feet, FeetPerMinute, Kelvin, Kilograms, KilogramsPerCubicMeter, Knots, Meters,
    MetersPerSecond, Newtons, Pascals, Radians, Seconds, SquareMeters,
};
