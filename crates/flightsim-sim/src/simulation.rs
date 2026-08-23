//! 描画ループから駆動するシミュレーション。
//!
//! # `fly` との違い
//!
//! [`crate::fly`] はシナリオを最後まで回して軌跡を返すバッチ実行。
//! こちらは**1 描画フレームぶんだけ進める**逐次実行で、Bevy 層はこれを呼ぶ。
//!
//! # なぜ Bevy 層で結線を再実装しないのか
//!
//! 地形 → 接地平面 → FDM の結線は [ADR-0006](../../../../docs/adr/0006-simulation-integration-layer.md)
//! でここに集約すると決めた。同じ結線が 2 箇所にあると、片方だけ直されて挙動が
//! 食い違う。**`flightsim-app` はこの型を持つだけにすること。**
//!
//! # 補間
//!
//! 物理は固定 dt（既定 120 Hz）で進み、描画は可変フレームレートで回る。
//! 端数ぶんは [`Simulation::interpolated`] が前ステップと現ステップを混ぜて返す。
//!
//! **補間結果を物理状態に書き戻さない。** 書き戻すと決定論が壊れ、
//! リプレイとネットワーク同期の前提が崩れる（ADR-0004）。
//! そのために補間結果は [`InterpolatedState`] という別の型にしてあり、
//! `RigidBodyState` として取り回せないようにしている。

use crate::ground::{GroundPlane, GroundSampler};
use flightsim_core::{
    Attitude, Ecef, FixedStep, Geodetic, Meters, MetersPerSecond, Radians, Seconds,
};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, Environment, FlightDynamics, RECOMMENDED_FIXED_DT,
    RigidBodyState,
};
use flightsim_world::{Terrain, TileSource};
use glam::DQuat;

/// 描画に使う補間済みの姿勢と位置。
///
/// **物理状態ではない。** `RigidBodyState` に戻せないのは意図的で、
/// 誤って物理へ書き戻す経路を型で塞いでいる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterpolatedState {
    /// 世界座標。
    pub position: Ecef,
    /// 機体軸 → ECEF の回転。
    pub orientation: DQuat,
    /// 測地座標。
    pub geodetic: Geodetic,
    /// ローカル基準の姿勢角。
    pub attitude: Attitude,
}

/// 接地の記録。
///
/// # 何のためか
///
/// 着陸を**評価**するため。接地の瞬間の沈下率・接地点・姿勢が取れないと、
/// 「うまく降りられたか」をプレイヤーに言えない。HUD の昇降率は表示の
/// 瞬間の値であって、接地の瞬間の値ではない。
///
/// 値は**接地直前の空中の状態**から取る。接地後の状態では、脚のばねが
/// すでに衝撃を吸収していて、沈下率が実際より穏やかに見える。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touchdown {
    /// 接地点。
    pub position: Geodetic,
    /// 接地時の降下率。**降下が正。** 上昇中の接地（バウンド）では負になる。
    pub sink_rate: MetersPerSecond,
    /// 接地時の対地速度（水平成分）。
    pub ground_speed: MetersPerSecond,
    /// 接地時のバンク角。
    pub bank: Radians,
    /// 接地時の機首方位。滑走路方位との揃いを見るのに使う。
    pub heading: Radians,
    /// シミュレーション開始からの時刻。
    pub elapsed: Seconds,
}

/// 1 フレーム進めた結果の報告。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StepReport {
    /// このフレームで実行した固定ステップ数。
    pub steps: u32,
    /// 状態が非有限になったため、進めるのをやめたか。
    ///
    /// **真になったら軌跡は信用できない。** 呼び出し側は必ず確認すること。
    pub diverged: bool,
    /// このフレームで地形データが引けなかったか。
    pub terrain_missing: bool,
}

/// 地形と FDM を結線して回す。
#[derive(Debug)]
pub struct Simulation<S: TileSource> {
    dynamics: FlightDynamics,
    terrain: Terrain<S>,
    sampler: GroundSampler,
    fixed: FixedStep,
    /// 補間の始点。物理を進める直前の状態。
    previous: RigidBodyState,
    ground: GroundPlane,
    diverged: bool,
    /// 車輪の最下点（機体基準）。接地判定に使う。
    gear_height: Meters,
    /// 空中にいるか。ヒステリシスつき（下記 `update_contact`）。
    airborne: bool,
    /// 最後に記録した接地。
    last_touchdown: Option<Touchdown>,
    /// 接地の通算回数。呼び出し側は前回読んだ値との差で「新しい接地」を知る。
    touchdown_count: u32,
}

impl<S: TileSource> Simulation<S> {
    /// 滑走路上に静止した状態で作る。
    #[must_use]
    pub fn parked(
        config: AircraftConfig,
        start: Geodetic,
        heading: flightsim_core::Radians,
        mut terrain: Terrain<S>,
        sampler: GroundSampler,
    ) -> Self {
        let ground = sampler.sample(&mut terrain, start);
        let state = crate::flight::parked_state(&config, start, ground.elevation, heading);
        let gear_height = crate::flight::gear_height(&config);
        Self {
            dynamics: FlightDynamics::new(config, state),
            terrain,
            sampler,
            fixed: FixedStep::new(RECOMMENDED_FIXED_DT),
            previous: state,
            ground,
            diverged: false,
            gear_height,
            // 駐機から始まるので接地済み。spawn の瞬間を着陸として数えない。
            airborne: false,
            last_touchdown: None,
            touchdown_count: 0,
        }
    }

    /// 空中の任意状態から作る。リプレイの巻き戻しやテレポートに使う。
    #[must_use]
    pub fn from_state(
        config: AircraftConfig,
        state: RigidBodyState,
        mut terrain: Terrain<S>,
        sampler: GroundSampler,
    ) -> Self {
        let ground = sampler.sample(&mut terrain, state.geodetic());
        let gear_height = crate::flight::gear_height(&config);
        let clearance = state.altitude().get() - ground.elevation.get() - gear_height.get();
        Self {
            dynamics: FlightDynamics::new(config, state),
            terrain,
            sampler,
            fixed: FixedStep::new(RECOMMENDED_FIXED_DT),
            previous: state,
            ground,
            diverged: false,
            gear_height,
            // 空中から始まれば、最初の接地も着陸として数える。
            airborne: clearance > crate::flight::AIRBORNE_CLEARANCE.get(),
            last_touchdown: None,
            touchdown_count: 0,
        }
    }

    /// 描画フレーム時間ぶん進める。
    ///
    /// 内部で固定 dt に分割する。**フレーム時間をそのまま物理へ渡さない**
    /// のがこのメソッドの役目（ADR-0004）。
    pub fn advance(&mut self, frame_time: Seconds, controls: ControlInputs) -> StepReport {
        if self.diverged {
            return StepReport {
                steps: 0,
                diverged: true,
                terrain_missing: !self.ground.from_terrain,
            };
        }

        let steps = self.fixed.advance(frame_time);
        let mut terrain_missing = false;

        for _ in 0..steps {
            let state = *self.dynamics.state();
            if !state.is_finite() {
                self.diverged = true;
                break;
            }
            self.previous = state;

            self.ground = self.sampler.sample(&mut self.terrain, state.geodetic());
            terrain_missing |= !self.ground.from_terrain;

            // 接地平面は 1 ステップの間固定される（ADR-0004）。
            let environment = Environment::still_air().with_ground_plane(
                self.ground.reference,
                self.ground.elevation,
                self.ground.slope,
            );
            self.dynamics
                .step(self.fixed.fixed_dt(), controls, &environment);
            self.update_contact();
        }

        if !self.dynamics.state().is_finite() {
            self.diverged = true;
        }

        StepReport {
            steps,
            diverged: self.diverged,
            terrain_missing,
        }
    }

    /// 接地状態を更新し、空中 → 接地の遷移を記録する。
    ///
    /// ヒステリシスを持たせている。接地の閾値（車輪隙間 0.05 m）と
    /// 「確実に空中」の閾値（0.5 m）を分けないと、滑走中の凹凸や
    /// 接地直後のバウンドで着陸が二重三重に数えられる。
    fn update_contact(&mut self) {
        /// 車輪の隙間がこれ以下なら接地とみなす。
        const CONTACT_CLEARANCE: Meters = Meters(0.05);

        let state = self.dynamics.state();
        let clearance =
            state.altitude().get() - self.ground.elevation.get() - self.gear_height.get();

        if self.airborne {
            if clearance <= CONTACT_CLEARANCE.get() {
                self.airborne = false;
                // **接地直前の空中の状態から取る。** 接地後では脚のばねが
                // 衝撃を吸収していて、沈下率が実際より穏やかに見える。
                let before = &self.previous;
                let attitude = before.attitude();
                self.last_touchdown = Some(Touchdown {
                    position: state.geodetic(),
                    // vertical_speed は上昇が正。降下を正にして返す。
                    sink_rate: MetersPerSecond(-before.vertical_speed().get()),
                    ground_speed: before.ground_speed(),
                    bank: attitude.roll,
                    heading: attitude.yaw,
                    elapsed: self.fixed.elapsed(),
                });
                self.touchdown_count = self.touchdown_count.saturating_add(1);
            }
        } else if clearance > crate::flight::AIRBORNE_CLEARANCE.get() {
            self.airborne = true;
        }
    }

    /// 最後に記録した接地。
    #[must_use]
    pub const fn last_touchdown(&self) -> Option<&Touchdown> {
        self.last_touchdown.as_ref()
    }

    /// 接地の通算回数。
    ///
    /// **前回読んだ値と比べて増えていたら新しい接地があった**、と読む。
    /// 「今のフレームで接地したか」を bool で返すと、フレームを跨いで
    /// 読み損ねたときに取りこぼす。
    #[must_use]
    pub const fn touchdown_count(&self) -> u32 {
        self.touchdown_count
    }

    /// 車輪が地面に着いているか。
    #[must_use]
    pub const fn on_ground(&self) -> bool {
        !self.airborne
    }

    /// 物理状態そのもの。**描画にはこれではなく [`Self::interpolated`] を使う。**
    #[must_use]
    pub const fn state(&self) -> &RigidBodyState {
        self.dynamics.state()
    }

    /// 描画用に補間した位置と姿勢。
    ///
    /// 姿勢は quaternion の球面線形補間。オイラー角を線形補間すると
    /// 方位が 359° → 1° をまたぐ瞬間に機体が一回転する。
    #[must_use]
    pub fn interpolated(&self) -> InterpolatedState {
        let alpha = self.fixed.interpolation_alpha();
        let current = self.dynamics.state();

        let position = Ecef::from_vec(
            self.previous
                .position
                .as_vec()
                .lerp(current.position.as_vec(), alpha),
        );
        let orientation = self
            .previous
            .orientation
            .slerp(current.orientation, alpha)
            .normalize();

        let geodetic = position.to_geodetic();
        let attitude = Attitude::from_quaternion(
            flightsim_core::LocalFrame::new(geodetic)
                .ned_to_ecef_rotation()
                .inverse()
                * orientation,
        );

        InterpolatedState {
            position,
            orientation,
            geodetic,
            attitude,
        }
    }

    /// 直近に評価した接地平面。
    #[must_use]
    pub const fn ground(&self) -> GroundPlane {
        self.ground
    }

    /// 重心の対地高度。
    #[must_use]
    pub fn agl(&self) -> Meters {
        Meters(self.dynamics.state().altitude().get() - self.ground.elevation.get())
    }

    #[must_use]
    pub const fn config(&self) -> &AircraftConfig {
        self.dynamics.config()
    }

    #[must_use]
    pub const fn terrain(&self) -> &Terrain<S> {
        &self.terrain
    }

    /// 地形へ可変アクセスする。描画側がタイルを引くために使う。
    pub const fn terrain_mut(&mut self) -> &mut Terrain<S> {
        &mut self.terrain
    }

    /// 経過したシミュレーション時間。
    #[must_use]
    pub const fn elapsed(&self) -> Seconds {
        self.fixed.elapsed()
    }

    /// 状態が非有限になったか。
    #[must_use]
    pub const fn diverged(&self) -> bool {
        self.diverged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Meters as M, Radians};
    use flightsim_world::dem::HeightGrid;
    use flightsim_world::{DemTile, MemoryTileSource, TileId};

    fn terrain_at(start: Geodetic, elevation: f64) -> Terrain<MemoryTileSource> {
        let id = TileId::containing(9, start);
        let mut source = MemoryTileSource::new();
        source.insert(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(33, 33, M(elevation))),
        );
        Terrain::new(source, 8 * 1024 * 1024, 9..=9)
    }

    fn parked() -> Simulation<MemoryTileSource> {
        let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
        Simulation::parked(
            AircraftConfig::light_single(),
            start,
            Radians::ZERO,
            terrain_at(start, 300.0),
            GroundSampler::default(),
        )
    }

    #[test]
    fn a_parked_aircraft_starts_on_the_terrain() {
        let simulation = parked();
        assert!((simulation.ground().elevation.get() - 300.0).abs() < 1.0);
        assert!(
            simulation.agl().get() > 0.5 && simulation.agl().get() < 1.5,
            "the parked aircraft sits {} above the ground",
            simulation.agl()
        );
    }

    #[test]
    fn a_frame_is_split_into_fixed_steps() {
        // 描画フレーム時間をそのまま物理へ渡さないのがこのメソッドの役目。
        let mut simulation = parked();
        let report = simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        assert_eq!(report.steps, 2, "60 Hz should give two 120 Hz steps");
        assert!(!report.diverged);
    }

    #[test]
    fn a_long_frame_is_clamped_rather_than_spiralling() {
        // 重いフレームが大量のステップを誘発すると death spiral に入る。
        let mut simulation = parked();
        let report = simulation.advance(Seconds(10.0), ControlInputs::neutral());
        assert!(
            report.steps <= 30,
            "a 10 s frame ran {} steps; the clamp is not working",
            report.steps
        );
    }

    #[test]
    fn the_interpolated_state_stays_between_the_two_physics_states() {
        let mut simulation = parked();
        // 離陸して動きのある状態にする。
        for _ in 0..600 {
            simulation.advance(
                Seconds(1.0 / 60.0),
                ControlInputs::new(0.0, 0.3, 0.0, 1.0, 0.0),
            );
        }
        assert!(!simulation.diverged());

        // 半端なフレーム時間で端数を作る。
        simulation.advance(
            Seconds(1.0 / 100.0),
            ControlInputs::new(0.0, 0.3, 0.0, 1.0, 0.0),
        );

        let interpolated = simulation.interpolated();
        let current = simulation.state().position.as_vec();
        assert!(
            interpolated.position.as_vec().distance(current) < 10.0,
            "the interpolated position is {} m from the physics state",
            interpolated.position.as_vec().distance(current)
        );
        assert!(interpolated.position.is_finite());
        assert!(interpolated.attitude.is_finite());
    }

    #[test]
    fn interpolation_does_not_write_back_into_the_physics_state() {
        // 書き戻すと決定論が壊れる（ADR-0004）。
        let mut simulation = parked();
        for _ in 0..120 {
            simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        }

        let before = *simulation.state();
        for _ in 0..50 {
            let _ = simulation.interpolated();
        }
        assert_eq!(
            &before,
            simulation.state(),
            "calling interpolated() changed the physics state"
        );
    }

    #[test]
    fn the_same_frame_sequence_produces_the_same_state() {
        // 逐次 API でも決定論が保たれること。
        let run = || {
            let mut simulation = parked();
            for index in 0..300 {
                // フレーム時間を意図的に揺らす。実際の描画はこうなる。
                let frame = Seconds(1.0 / 60.0 + f64::from(index % 7) * 1e-4);
                simulation.advance(frame, ControlInputs::new(0.1, 0.2, 0.0, 0.9, 0.0));
            }
            *simulation.state()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_missing_tile_is_reported() {
        let start = Geodetic::from_degrees(0.0, -150.0, 0.0);
        let mut simulation = Simulation::parked(
            AircraftConfig::light_single(),
            start,
            Radians::ZERO,
            Terrain::new(MemoryTileSource::new(), 1024 * 1024, 9..=9),
            GroundSampler::default(),
        );
        let report = simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        assert!(report.terrain_missing);
        assert!(!report.diverged);
    }

    #[test]
    fn advancing_after_divergence_is_a_no_op() {
        let mut simulation = parked();
        // 非有限な状態を直接注入する。
        let mut broken = *simulation.state();
        broken.velocity = glam::DVec3::new(f64::NAN, 0.0, 0.0);
        simulation.dynamics.set_state(broken);

        let report = simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        assert!(report.diverged);
        let second = simulation.advance(Seconds(1.0 / 60.0), ControlInputs::neutral());
        assert_eq!(second.steps, 0, "a diverged simulation kept stepping");
    }
}
