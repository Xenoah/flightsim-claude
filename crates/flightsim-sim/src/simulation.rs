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
    Attitude, Ecef, FixedStep, Geodetic, Meters, MetersPerSecond, Ned, Radians, Seconds,
};
use flightsim_fdm::{
    AircraftConfig, Atmosphere, ControlInputs, Environment, FlightDynamics, RECOMMENDED_FIXED_DT,
    RigidBodyState, Turbulence,
};
use flightsim_world::{
    ClimateDataError, ClimateDate, ClimateSample, GlobalClimate, Terrain, TileSource,
};
use glam::DQuat;

/// Physical air for a prospective start point, using the same world/FDM bridge
/// as [`Simulation`]. Intended for density-aware initialization before a
/// simulation exists; it only reads the validated immutable climate snapshot.
///
/// # Errors
/// Returns an error if enabled bundled climate data fail validation.
pub fn climate_atmosphere_sample(
    position: Geodetic,
    date: Option<ClimateDate>,
) -> Result<flightsim_fdm::AtmosphereSample, ClimateDataError> {
    let atmosphere = if let Some(date) = date {
        atmosphere_from_climate(GlobalClimate::bundled()?.sample(position, date))
    } else {
        Atmosphere::standard()
    };
    Ok(atmosphere.sample(position.altitude))
}

fn atmosphere_from_climate(sample: ClimateSample) -> Atmosphere {
    Atmosphere::with_temperature_offset(sample.isa_temperature_offset.get())
}

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

/// 定常風。
///
/// 航空の慣習どおり「どちら**から**吹くか」で持つ（270/10 = 西から 10 kt）。
/// METAR も管制も風をこの向きで言う。NED ベクトルへの変換はここが引き受け、
/// **符号の取り違え（from/to の逆転）をこの型の外に漏らさない。**
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wind {
    /// 風が吹いてくる真方位。
    pub from: Radians,
    /// 風速。
    pub speed: MetersPerSecond,
}

impl Wind {
    /// 無風。
    pub const CALM: Self = Self {
        from: Radians::ZERO,
        speed: MetersPerSecond(0.0),
    };

    /// NED の風ベクトル（空気が動いていく向き）。
    ///
    /// 270°（西）**から** 10 m/s は、空気が**東へ** 10 m/s 動くこと。
    #[must_use]
    pub fn to_ned(self) -> Ned {
        let speed = if self.speed.get().is_finite() {
            self.speed.get().max(0.0)
        } else {
            0.0
        };
        let (sin, cos) = self.from.get().sin_cos();
        // from の反対向きへ動く: to = from + 180° なので成分は符号反転。
        Ned::new(-cos * speed, -sin * speed, 0.0)
    }
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

/// 飛行の記録。
///
/// # なぜ要るのか
///
/// 1 回の着陸の良し悪しだけでは「今日の飛行はどうだったか」が言えない。
/// **プレイヤーが続けたくなるには、積み上がるものが要る。**
///
/// 距離は大円距離の**累積**であって、出発点からの直線距離ではない。
/// 場周を 1 周すれば出発点に戻るが、飛んだ距離は 0 ではない。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlightLog {
    /// 空中にいた時間の合計。
    pub airborne_time: Seconds,
    /// 飛んだ距離の累積。
    pub distance: Meters,
    /// 到達した最高の対地高度。
    pub peak_agl: Meters,
    /// 記録した最高の対気速度。
    pub peak_airspeed: MetersPerSecond,
    /// 接地の回数。
    pub landings: u32,
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
    /// 消化した物理ステップの時刻。フレーム単位で先に進む `fixed.elapsed()` を
    /// 乱流に使うと、同じ入力でも描画フレームの割り方で突風が変わってしまう。
    /// 1 ステップずつ同じ順序で加算し、浮動小数点の丸めもフレームから独立させる。
    physics_elapsed: Seconds,
    /// 補間の始点。物理を進める直前の状態。
    previous: RigidBodyState,
    ground: GroundPlane,
    diverged: bool,
    /// 定常風。
    wind: Wind,
    /// 乱流。既定は無乱流で、既存の呼び出しの挙動は変わらない。
    turbulence: Turbulence,
    /// Validated, immutable monthly climatology and a fixed session date.
    /// None preserves the original ISA-only physics and version-1 replays.
    climate: Option<(GlobalClimate, ClimateDate)>,
    /// 飛行の記録。
    log: FlightLog,
    /// 距離の累積に使う直前の位置。
    previous_position: Geodetic,
    /// 車輪の最下点（機体基準）。接地判定に使う。
    gear_height: Meters,
    /// 空中にいるか。ヒステリシスつき（下記 `update_contact`）。
    airborne: bool,
    /// 最後に記録した接地。
    last_touchdown: Option<Touchdown>,
    /// 接地の通算回数。呼び出し側は前回読んだ値との差で「新しい接地」を知る。
    touchdown_count: u32,
    /// 墜落と判定する境界。
    crash_limits: crate::CrashLimits,
    /// 墜落したならその記録。**あれば以降は進めない。**
    crash: Option<crate::Crash>,
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
        let state =
            crate::flight::parked_state(&config, start, ground.elevation, ground.slope, heading);
        let gear_height = crate::flight::gear_height(&config);
        Self {
            dynamics: FlightDynamics::new(config, state),
            terrain,
            sampler,
            fixed: FixedStep::new(RECOMMENDED_FIXED_DT),
            physics_elapsed: Seconds::ZERO,
            previous: state,
            ground,
            diverged: false,
            wind: Wind::CALM,
            turbulence: Turbulence::CALM,
            climate: None,
            log: FlightLog::default(),
            previous_position: state.geodetic(),
            gear_height,
            // 駐機から始まるので接地済み。spawn の瞬間を着陸として数えない。
            airborne: false,
            last_touchdown: None,
            touchdown_count: 0,
            crash_limits: crate::CrashLimits::default(),
            crash: None,
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
            physics_elapsed: Seconds::ZERO,
            previous: state,
            ground,
            diverged: false,
            wind: Wind::CALM,
            turbulence: Turbulence::CALM,
            climate: None,
            log: FlightLog::default(),
            previous_position: state.geodetic(),
            gear_height,
            // 空中から始まれば、最初の接地も着陸として数える。
            airborne: clearance > crate::flight::AIRBORNE_CLEARANCE.get(),
            last_touchdown: None,
            touchdown_count: 0,
            crash_limits: crate::CrashLimits::default(),
            crash: None,
        }
    }

    /// 状態を差し替えて、そこから続きを回す。
    ///
    /// リプレイの後退シーク用。地形を作り直さずに済むので、
    /// [`Self::from_state`] のように `Terrain` を持ち出す必要がない。
    ///
    /// # 何を戻して、何を戻さないか
    ///
    /// 戻すのは**物理の状態と接地の追跡**（姿勢・速度・接地平面・空中判定・
    /// 発散フラグ・補間の始点）。
    ///
    /// **飛行記録（[`Self::log`]）と接地回数は戻さない。** これらはセッションを
    /// 通した積み上げで、巻き戻したからといって飛んだ事実は消えない。
    /// リプレイの表示に使うのは記録側の値であって、ここではない。
    ///
    /// 固定ステップのアキュムレータも戻さない。端数が残ったまま続くので、
    /// 通しで飛んだ場合とサブステップの割れ方がわずかに変わる
    /// （実測 0.17 m / 8.7 秒。`tests/replay_fidelity.rs`）。
    pub fn rewind_to(&mut self, state: RigidBodyState) {
        self.dynamics.set_state(state);
        self.previous = state;
        self.ground = self.sampler.sample(&mut self.terrain, state.geodetic());
        self.previous_position = state.geodetic();
        let clearance =
            state.altitude().get() - self.ground.elevation.get() - self.gear_height.get();
        self.airborne = clearance > crate::flight::AIRBORNE_CLEARANCE.get();
        // 発散した状態から巻き戻すのは、まさに発散から抜けたいとき。
        self.diverged = false;
        // 墜落も同じ。**壊れたまま巻き戻しても何もできない。**
        self.crash = None;
    }

    /// 最初からやり直す。**記録も消える。**
    ///
    /// [`Self::rewind_to`] との違いは、飛行記録・接地回数・固定ステップの
    /// アキュムレータまで初期化すること。
    ///
    /// # なぜ記録まで消すのか
    ///
    /// やり直しは「さっきの飛行は無かったことにする」という意思。
    /// 積み上げを残すと、失敗を繰り返すほど飛行距離と接地回数が増え、
    /// **記録が「何回やり直したか」を測る数字になってしまう。**
    ///
    /// リプレイの記録側も併せて捨てること。やり直しは記録に残らないので、
    /// 残したまま続けると、再生時に同じ入力を流しても別の飛行になる。
    pub fn restart_at(&mut self, state: RigidBodyState) {
        self.rewind_to(state);
        self.log = FlightLog::default();
        self.last_touchdown = None;
        self.touchdown_count = 0;
        self.fixed = FixedStep::new(RECOMMENDED_FIXED_DT);
        self.physics_elapsed = Seconds::ZERO;
    }

    /// 滑走路上の静止状態からやり直す。
    ///
    /// 地形は持っているものをそのまま使い、標高と勾配だけ引き直す。
    /// [`Self::parked`] と同じ姿勢になる。
    pub fn restart_parked_at(&mut self, start: Geodetic, heading: flightsim_core::Radians) {
        let ground = self.sampler.sample(&mut self.terrain, start);
        let state = crate::flight::parked_state(
            self.dynamics.config(),
            start,
            ground.elevation,
            ground.slope,
            heading,
        );
        self.restart_at(state);
    }

    /// Advance a render frame using the same controls for every fixed step.
    ///
    /// This is the constant-input wrapper around [`Self::advance_with_controls`].
    /// The frame duration is never passed directly to the FDM (ADR-0004).
    pub fn advance(&mut self, frame_time: Seconds, controls: ControlInputs) -> StepReport {
        self.advance_with_controls(frame_time, |_, _| controls)
    }

    /// Advance a render frame, obtaining controls immediately before each FDM step.
    ///
    /// The callback receives the fixed physics duration (120 Hz) and the **pre-step**
    /// state. Update rate-dependent pilot controls there, and pass that same duration,
    /// control value and state to [`crate::replay::Recorder::record`] when recording.
    /// The existing [`FixedStep`] accumulator alone decides how many steps are due;
    /// sub-step render frames do not call the callback. Paused callers should omit
    /// this call or pass zero time, so no paused time is accumulated.
    ///
    /// A crash or divergence stops the frame immediately. The report counts only
    /// executed FDM steps, including a step that diverges. The unused frame budget
    /// and fractional remainder are discarded on that terminal outcome, while
    /// [`Self::elapsed`] retains only the time of executed steps. A frozen simulation
    /// never calls the callback, records another input, or catches up discarded time.
    pub fn advance_with_controls(
        &mut self,
        frame_time: Seconds,
        mut controls_for_step: impl FnMut(Seconds, &RigidBodyState) -> ControlInputs,
    ) -> StepReport {
        if self.crash.is_some() || self.diverged {
            return StepReport {
                steps: 0,
                diverged: self.diverged,
                terrain_missing: !self.ground.from_terrain,
            };
        }

        let due = self.fixed.advance(frame_time);
        let dt = self.fixed.fixed_dt();
        let mut steps = 0;
        let mut terrain_missing = false;

        for _ in 0..due {
            let state = *self.dynamics.state();
            if !state.is_finite() {
                self.diverged = true;
                break;
            }
            self.previous = state;

            self.ground = self.sampler.sample(&mut self.terrain, state.geodetic());
            terrain_missing |= !self.ground.from_terrain;

            // Sample the environment at the step endpoint, exactly as in repeated
            // 120 Hz calls. Time and addition order are independent of render cadence.
            self.physics_elapsed += dt;
            let environment = self.environment_for(&state);
            let controls = controls_for_step(dt, &state);
            self.dynamics.step(dt, controls, &environment);
            steps += 1;

            if !self.dynamics.state().is_finite() {
                self.diverged = true;
                break;
            }
            self.update_contact();
            self.update_log();
            if self.crash.is_some() {
                // Freeze at the actual impact pose, not a render-frame-dependent
                // blend of the last two states. This never changes physical state.
                self.previous = *self.dynamics.state();
                break;
            }
        }

        if !self.dynamics.state().is_finite() {
            self.diverged = true;
        }
        if self.crash.is_some() || self.diverged {
            // FixedStep reserves the whole frame up front. No reserved step or
            // fractional remainder may survive a terminal stop; physical elapsed
            // above, rather than FixedStep's reserved elapsed, is authoritative.
            self.fixed.reset();
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
                    elapsed: self.physics_elapsed,
                });
                self.touchdown_count = self.touchdown_count.saturating_add(1);
                self.log.landings = self.touchdown_count;

                // **接地の記録は墜落でも残す。** 何が起きたかを見るのに要る。
                if let Some(cause) = self.crash_limits.evaluate(
                    MetersPerSecond(-before.vertical_speed().get()),
                    attitude.roll,
                    attitude.pitch,
                ) {
                    self.crash = Some(crate::Crash {
                        cause,
                        position: state.geodetic(),
                        elapsed: self.physics_elapsed,
                    });
                }
            }
        } else if clearance > crate::flight::AIRBORNE_CLEARANCE.get() {
            self.airborne = true;
        }
    }

    /// 1 ステップぶん記録を更新する。
    ///
    /// **非有限値を記録に混ぜない。** 一度混ざると以降の最大値が
    /// 全部 NaN になり、記録が読めなくなる。
    fn update_log(&mut self) {
        let state = self.dynamics.state();
        let position = state.geodetic();

        let step = self.previous_position.great_circle_distance(position).get();
        if step.is_finite() {
            self.log.distance = Meters(self.log.distance.get() + step);
        }
        self.previous_position = position;

        if self.airborne {
            self.log.airborne_time =
                Seconds(self.log.airborne_time.get() + self.fixed.fixed_dt().get());
        }

        let agl = state.altitude().get() - self.ground.elevation.get();
        if agl.is_finite() && agl > self.log.peak_agl.get() {
            self.log.peak_agl = Meters(agl);
        }

        let airspeed = self.airspeed().get();
        if airspeed.is_finite() && airspeed > self.log.peak_airspeed.get() {
            self.log.peak_airspeed = MetersPerSecond(airspeed);
        }
    }

    /// 飛行の記録。
    #[must_use]
    pub const fn log(&self) -> FlightLog {
        self.log
    }

    /// 真対気速度。
    ///
    /// **対地速度ではない。** 風が入ると両者は一致しない
    /// （向かい風 10 m/s の中を対地 30 m/s で走れば対気は 40 m/s）。
    /// 失速も揚力も対気速度で決まるので、計器に出すのはこちら。
    #[must_use]
    pub fn airspeed(&self) -> MetersPerSecond {
        // 計器も空力・失速判定と同じ瞬間の定常風と乱流を差し引く。
        self.aero_angles().true_airspeed
    }

    /// 乱流を設定する。
    pub const fn set_turbulence(&mut self, turbulence: Turbulence) {
        self.turbulence = turbulence;
    }

    /// 現在の乱流。
    #[must_use]
    pub const fn turbulence(&self) -> Turbulence {
        self.turbulence
    }

    /// 定常風を設定する。
    pub const fn set_wind(&mut self, wind: Wind) {
        self.wind = wind;
    }

    /// 現在の定常風。
    #[must_use]
    pub const fn wind(&self) -> Wind {
        self.wind
    }

    /// Enable a fixed seasonal climate date, or return to ISA with `None`.
    ///
    /// The bundled atlas is validated on first enable and shared thereafter.
    /// Changing the date changes physics; callers recording a flight must
    /// start a new session and save it in replay conditions before advancing.
    /// No wall time or visual-clock time acceleration is used here.
    ///
    /// # Errors
    /// Returns an error if the bundled atlas cannot be validated. Existing
    /// climate settings remain unchanged when loading fails.
    pub fn set_climate(&mut self, date: Option<ClimateDate>) -> Result<(), ClimateDataError> {
        match date {
            None => self.climate = None,
            Some(date) => {
                if let Some((_, existing)) = &mut self.climate {
                    *existing = date;
                } else {
                    self.climate = Some((GlobalClimate::bundled()?, date));
                }
            }
        }
        Ok(())
    }

    /// Fixed climatological date used for this flight; `None` means ISA.
    #[must_use]
    pub fn climate(&self) -> Option<ClimateDate> {
        self.climate.as_ref().map(|(_, date)| *date)
    }

    /// Climate at the aircraft position, with explicit reanalysis provenance.
    /// `temperature` is the climatology's approximate tropospheric lapse;
    /// [`Self::atmosphere_sample`] reports the exact ISA-offset physical air.
    #[must_use]
    pub fn climate_sample(&self) -> Option<ClimateSample> {
        self.climate
            .as_ref()
            .map(|(climate, date)| climate.sample(self.dynamics.state().geodetic(), *date))
    }

    /// Actual local temperature, pressure and density used by flight dynamics.
    /// With climate enabled, pressure stays ISA while regional temperature
    /// changes density and speed of sound. This does not simulate live QNH.
    #[must_use]
    pub fn atmosphere_sample(&self) -> flightsim_fdm::AtmosphereSample {
        let position = self.dynamics.state().geodetic();
        self.atmosphere_for(position).sample(position.altitude)
    }

    fn atmosphere_for(&self, position: Geodetic) -> Atmosphere {
        self.climate
            .as_ref()
            .map_or_else(Atmosphere::standard, |(climate, date)| {
                atmosphere_from_climate(climate.sample(position, *date))
            })
    }

    /// このステップの環境（大気・風・乱流・接地平面）。
    ///
    /// **積分と、外から状態を読むときで同じものを使う。** 2 箇所に書くと
    /// 片方だけ直されて、警報が鳴るのと実際に失速するのがずれる。
    fn environment_for(&self, state: &RigidBodyState) -> Environment {
        // 接地平面は 1 ステップの間固定される（ADR-0004）。風も同じく
        // ステップ間で固定（決定論。乱流や突風を入れるなら決定論的な
        // 擬似乱数列で、ここではなく Wind の生成側で行う）。
        // 乱流はシミュレーション時刻の関数。**壁時計ではない。**
        // 1 ステップの間は固定され、RK4 の中間評価で値が変わらない。
        Environment::with_wind_ned(
            self.atmosphere_for(state.geodetic()),
            state.geodetic(),
            self.wind.to_ned(),
        )
        .with_turbulence(self.turbulence, self.physics_elapsed, state.geodetic())
        .with_ground_plane(
            self.ground.reference,
            self.ground.elevation,
            self.ground.slope,
        )
    }

    /// 今の空力角（迎角・横滑り角・真対気速度）。
    ///
    /// 風と乱流を含んだ相対風から求める。**対地速度からではない。**
    #[must_use]
    pub fn aero_angles(&self) -> flightsim_fdm::AeroAngles {
        let state = self.dynamics.state();
        flightsim_fdm::aero_angles_of(state, &self.environment_for(state))
    }

    /// 失速までの余裕。迎角が失速角の何割まで来たかを `[0, 1]` で返す。
    ///
    /// 1.0 で失速角ちょうど。**それ以上でも 1.0 に頭打ちしない**ので、
    /// 呼び出し側は 1 を超えた値を見て「もう失速している」と判断できる。
    ///
    /// 対気速度がほぼ 0 のときは 0 を返す。**駐機中に警報を鳴らさない。**
    /// 静止時の迎角は定義できず、`aero_angles` も 0 を返す。
    #[must_use]
    pub fn stall_fraction(&self) -> f64 {
        let angles = self.aero_angles();
        let stall = self.dynamics.config().aero.stall_angle.get().abs();
        if !angles.is_finite() || stall <= f64::EPSILON {
            return 0.0;
        }
        // 迎角が十分に定義できる速度でだけ見る。**低速では
        // わずかな速度成分で迎角が跳ね、警報がちらつく。**
        if angles.true_airspeed.get() < 5.0 {
            return 0.0;
        }
        angles.angle_of_attack.get().abs() / stall
    }

    /// 墜落したならその記録。**あれば機体はもう進まない。**
    #[must_use]
    pub const fn crash(&self) -> Option<&crate::Crash> {
        self.crash.as_ref()
    }

    /// 墜落したか。
    #[must_use]
    pub const fn crashed(&self) -> bool {
        self.crash.is_some()
    }

    /// 墜落と判定する境界を差し替える。
    ///
    /// **難易度で変えないこと。** 理由は [`crate::crash`] の doc にある。
    /// 回帰テストで機体を壊したくないときは [`crate::CrashLimits::NONE`]。
    pub const fn set_crash_limits(&mut self, limits: crate::CrashLimits) {
        self.crash_limits = limits;
    }

    /// 現在の墜落判定の境界。
    #[must_use]
    pub const fn crash_limits(&self) -> crate::CrashLimits {
        self.crash_limits
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
        self.physics_elapsed
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
