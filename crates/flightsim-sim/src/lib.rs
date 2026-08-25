//! # flightsim-sim
//!
//! 地形と飛行力学を結線し、固定ステップで回すヘッドレス統合層。
//!
//! ## なぜ別クレートなのか
//!
//! `flightsim-fdm` は `flightsim-world` を参照できない（[ARCHITECTURE.md §2]）。
//! FDM が地形を自分で引きに行かないからこそ、FDM 単体のテストがデータなしで回る。
//! その規約を守ったまま両者を繋ぐ場所がここ（[ADR-0006]）。
//!
//! ```text
//!                 app (Bevy)
//!                     │
//!         render / input / ui        ← Bevy 依存層
//!                     │
//!                    sim             ← このクレート。純 Rust
//!               ┌─────┴─────┐
//!             world        fdm       ← 純 Rust
//!               └─────┬─────┘
//!                    core
//! ```
//!
//! **M2 で Bevy 層が乗る際も、この結線を再実装しないこと。** 同じ結線が 2 箇所に
//! あると、片方だけ直されて挙動が食い違う。
//!
//! ## 構成
//!
//! | モジュール | 役割 |
//! |---|---|
//! | [`ground`] | 地形から接地平面（基準点・標高・勾配）を作る |
//! | [`director`] | 決定論的な PD フライトディレクタ。回帰テストの駆動装置 |
//! | [`flight`] | フェーズ遷移、固定ステップ駆動、軌跡の記録（バッチ実行） |
//! | [`simulation`] | 1 描画フレームぶんだけ進める逐次 API。Bevy 層はこれを呼ぶ |
//!
//! ## 使い方
//!
//! ```
//! use flightsim_core::{Geodetic, Meters};
//! use flightsim_fdm::AircraftConfig;
//! use flightsim_sim::{CircuitPlan, GroundSampler, SimulationOptions, fly};
//! use flightsim_world::{MemoryTileSource, Terrain};
//!
//! // タイルが無ければ楕円体高 0 m の海面として扱われる。
//! let mut terrain = Terrain::new(MemoryTileSource::new(), 64 * 1024 * 1024, 8..=12);
//!
//! let trajectory = fly(
//!     &AircraftConfig::light_single(),
//!     &CircuitPlan::default(),
//!     Geodetic::from_degrees(35.55, 139.78, 0.0),
//!     &mut terrain,
//!     &GroundSampler::default(),
//!     &SimulationOptions::default(),
//! );
//!
//! assert!(!trajectory.diverged, "the trajectory must stay finite");
//! assert!(trajectory.peak_agl() > Meters(100.0), "the aircraft should get airborne");
//! ```
//!
//! [ARCHITECTURE.md §2]: https://github.com/Xenoah/flightsim-claude/blob/main/ARCHITECTURE.md
//! [ADR-0006]: https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0006-simulation-integration-layer.md

pub mod director;
pub mod flight;
pub mod ground;
pub mod simulation;

pub use director::{DirectorGains, DirectorTargets, FlightDirector, VerticalTarget};
pub use flight::{
    CircuitPlan, Phase, SimulationOptions, Trajectory, TrajectorySample, approach_state, fly,
    gear_height, parked_state,
};
pub use ground::{GroundPlane, GroundSampler};
pub use simulation::{FlightLog, InterpolatedState, Simulation, StepReport, Touchdown, Wind};
