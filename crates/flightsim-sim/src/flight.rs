//! 固定ステップで飛行を回し、軌跡を記録する。
//!
//! # 何をしているか
//!
//! ```text
//!   毎ステップ:
//!     1. 機体直下の地形から接地平面を作る      (ground.rs)
//!     2. Environment に詰めて FDM へ渡す        (flightsim-fdm)
//!     3. フェーズに応じた目標を決める            (このモジュール)
//!     4. フライトディレクタが舵角を出す          (director.rs)
//!     5. FDM を dt 進める
//! ```
//!
//! **接地平面は 1 ステップの間固定される。** これは ADR-0004 の契約
//! （RK4 の中間状態でも同じ局所平面を使う）に合わせたもの。
//!
//! # 高度の呼び方
//!
//! - `altitude` — 楕円体高。FDM の世界座標そのもの
//! - `agl` — 重心の対地高度。`altitude - 地形標高`
//! - `wheel_clearance` — 車輪の対地高度。`agl - 脚の長さ`。接地判定はこちら
//!
//! 重心と車輪を混同すると、脚の長さぶん（この機体で 1 m）ずれた判定になる。

use crate::director::{DirectorTargets, FlightDirector, VerticalTarget};
use crate::ground::{GroundPlane, GroundSampler};
use flightsim_core::{Attitude, Geodetic, Meters, MetersPerSecond, Ned, Radians, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, Environment, FlightDynamics, RECOMMENDED_FIXED_DT,
    RigidBodyState,
};
use flightsim_world::{Terrain, TileSource};

/// 車輪がこれ以上地面から離れていれば、確実に空中にいるとみなす高さ。
///
/// 脚の最大ストロークが 0.25 m なので、それを明確に上回る値にしてある。
const AIRBORNE_CLEARANCE: Meters = Meters(0.5);

/// 飛行のフェーズ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// スロットル全開で滑走。
    TakeoffRoll,
    /// 一定ピッチで上昇。
    Climb,
    /// 場周高度で直進。
    Cruise,
    /// 指定方位へ旋回。
    Turn,
    /// 一定降下率で進入。
    Approach,
    /// 引き起こして接地。
    Flare,
    /// 接地後の減速。
    Rollout,
    /// 完了。
    Complete,
}

impl Phase {
    /// CSV や表示に使う短い名前。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TakeoffRoll => "takeoff_roll",
            Self::Climb => "climb",
            Self::Cruise => "cruise",
            Self::Turn => "turn",
            Self::Approach => "approach",
            Self::Flare => "flare",
            Self::Rollout => "rollout",
            Self::Complete => "complete",
        }
    }
}

/// 場周飛行の計画。
///
/// 既定値は `AircraftConfig::light_single` 向け。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircuitPlan {
    /// 滑走路の方位（真方位）。離陸と上昇の進行方向。
    pub runway_heading: Radians,
    /// 引き起こしを始める対気速度。
    pub rotate_speed: MetersPerSecond,
    /// 上昇中の目標ピッチ。
    pub climb_pitch: Radians,
    /// 場周高度（重心の対地高度）。
    pub pattern_altitude_agl: Meters,
    /// 場周高度での目標対気速度。
    pub cruise_speed: MetersPerSecond,
    /// 旋回を始めるまでの直進時間。
    pub cruise_duration: Seconds,
    /// 旋回後の方位。
    pub outbound_heading: Radians,
    /// 進入時の目標対気速度。
    pub approach_speed: MetersPerSecond,
    /// 進入時の降下率（正が降下）。
    pub approach_descent: MetersPerSecond,
    /// 進入時のフラップ。
    pub approach_flaps: f64,
    /// フレアを始める車輪の対地高度。
    pub flare_clearance: Meters,
    /// フレア中の目標降下率（正が降下）。
    ///
    /// **ピッチを直接指定しないこと。** スロットル全閉で機首を上げると余計に減速し、
    /// かえって沈下率が増える。実測で 3.07 m/s に対し、降下率保持なら 1.82 m/s。
    pub flare_descent: MetersPerSecond,
    /// 接地とみなす車輪の対地高度。
    pub touchdown_clearance: Meters,
    /// 接地後に減速する時間。
    pub rollout_duration: Seconds,
}

impl Default for CircuitPlan {
    fn default() -> Self {
        Self {
            runway_heading: Radians::ZERO,
            rotate_speed: MetersPerSecond(30.0),
            climb_pitch: Radians(8.0_f64.to_radians()),
            pattern_altitude_agl: Meters(300.0),
            cruise_speed: MetersPerSecond(50.0),
            cruise_duration: Seconds(40.0),
            outbound_heading: Radians(90.0_f64.to_radians()),
            approach_speed: MetersPerSecond(35.0),
            approach_descent: MetersPerSecond(3.0),
            approach_flaps: 1.0,
            flare_clearance: Meters(8.0),
            flare_descent: MetersPerSecond(0.5),
            touchdown_clearance: Meters(0.15),
            rollout_duration: Seconds(20.0),
        }
    }
}

impl CircuitPlan {
    /// フェーズに対応するディレクタへの指示。
    #[must_use]
    fn targets(&self, phase: Phase) -> DirectorTargets {
        let base = DirectorTargets {
            vertical: VerticalTarget::Pitch(Radians::ZERO),
            heading: self.runway_heading,
            airspeed: self.cruise_speed,
            flaps: 0.0,
            brakes: 0.0,
            throttle_override: None,
            wings_level: false,
        };

        match phase {
            // 地上では翼端を擦らないようバンクを禁じる。
            Phase::TakeoffRoll => DirectorTargets {
                throttle_override: Some(1.0),
                wings_level: true,
                ..base
            },
            Phase::Climb => DirectorTargets {
                vertical: VerticalTarget::Pitch(self.climb_pitch),
                throttle_override: Some(1.0),
                ..base
            },
            Phase::Cruise => DirectorTargets {
                vertical: VerticalTarget::AltitudeAgl(self.pattern_altitude_agl),
                ..base
            },
            Phase::Turn => DirectorTargets {
                vertical: VerticalTarget::AltitudeAgl(self.pattern_altitude_agl),
                heading: self.outbound_heading,
                ..base
            },
            Phase::Approach => DirectorTargets {
                vertical: VerticalTarget::DescentRate(self.approach_descent),
                heading: self.outbound_heading,
                airspeed: self.approach_speed,
                flaps: self.approach_flaps,
                ..base
            },
            // フレアはスロットルを絞り、低い沈下率を保つ。
            Phase::Flare => DirectorTargets {
                vertical: VerticalTarget::DescentRate(self.flare_descent),
                heading: self.outbound_heading,
                airspeed: self.approach_speed,
                flaps: self.approach_flaps,
                throttle_override: Some(0.0),
                ..base
            },
            Phase::Rollout => DirectorTargets {
                heading: self.outbound_heading,
                flaps: self.approach_flaps,
                brakes: 1.0,
                throttle_override: Some(0.0),
                wings_level: true,
                ..base
            },
            Phase::Complete => DirectorTargets {
                brakes: 1.0,
                throttle_override: Some(0.0),
                wings_level: true,
                ..base
            },
        }
    }

    /// フェーズの遷移判定。
    fn next_phase(&self, phase: Phase, elapsed_in_phase: f64, sample: &Snapshot) -> Phase {
        match phase {
            // 速度が回転速度に達したら引き起こす。
            //
            // それとは別に、**浮いてしまったら無条件に上昇へ移す**。零迎角でも
            // 揚力係数は正なので、機体は引き起こさなくても速度だけで浮き上がる。
            // 回転速度を高く設定しすぎた場合にそれが起き、実測では滑走のつもりのまま
            // 146 m まで上昇していた。TakeoffRoll は翼を水平に固定し高度も見ないため、
            // 空中にいるのに誰も機体を管理していない状態になる。
            Phase::TakeoffRoll
                if sample.airspeed.get() >= self.rotate_speed.get()
                    || sample.wheel_clearance.get() > AIRBORNE_CLEARANCE.get() =>
            {
                Phase::Climb
            }
            Phase::Climb if sample.agl.get() >= self.pattern_altitude_agl.get() => Phase::Cruise,
            Phase::Cruise if elapsed_in_phase >= self.cruise_duration.get() => Phase::Turn,
            Phase::Turn
                if sample
                    .heading
                    .shortest_difference_to(self.outbound_heading)
                    .get()
                    .abs()
                    < 5.0_f64.to_radians() =>
            {
                Phase::Approach
            }
            Phase::Approach if sample.wheel_clearance.get() <= self.flare_clearance.get() => {
                Phase::Flare
            }
            Phase::Flare if sample.wheel_clearance.get() <= self.touchdown_clearance.get() => {
                Phase::Rollout
            }
            Phase::Rollout if elapsed_in_phase >= self.rollout_duration.get() => Phase::Complete,
            unchanged => unchanged,
        }
    }
}

/// 1 ステップ分の観測量。フェーズ判定と記録の両方で使う。
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    airspeed: MetersPerSecond,
    agl: Meters,
    wheel_clearance: Meters,
    heading: Radians,
}

/// 軌跡の 1 点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrajectorySample {
    pub time: Seconds,
    pub phase: Phase,
    pub position: Geodetic,
    /// 地形標高（楕円体高）。
    pub ground_elevation: Meters,
    /// 重心の対地高度。
    pub agl: Meters,
    /// 車輪の対地高度。
    pub wheel_clearance: Meters,
    pub attitude: Attitude,
    pub airspeed: MetersPerSecond,
    pub ground_speed: MetersPerSecond,
    /// 上向きが正。
    pub vertical_speed: MetersPerSecond,
    pub controls: ControlInputs,
    /// 地形データから標高が得られたか。`false` なら海面を仮定している。
    pub terrain_available: bool,
}

/// 飛行の記録。
#[derive(Debug, Clone, PartialEq)]
pub struct Trajectory {
    pub samples: Vec<TrajectorySample>,
    /// 最終フェーズ。`Complete` でなければ時間切れか発散。
    pub final_phase: Phase,
    pub duration: Seconds,
    /// 状態が非有限になって打ち切った場合に真。
    ///
    /// **これが真なら軌跡は信用できない。** 呼び出し側は必ず確認すること。
    pub diverged: bool,
    /// 地形が引けなかったステップ数。
    pub steps_without_terrain: u64,
}

impl Trajectory {
    /// 通過したフェーズの一覧（重複を畳んだもの）。
    #[must_use]
    pub fn phases_visited(&self) -> Vec<Phase> {
        let mut phases: Vec<Phase> = Vec::new();
        for sample in &self.samples {
            if phases.last() != Some(&sample.phase) {
                phases.push(sample.phase);
            }
        }
        phases
    }

    /// 到達した最高の対地高度。
    #[must_use]
    pub fn peak_agl(&self) -> Meters {
        Meters(
            self.samples
                .iter()
                .map(|sample| sample.agl.get())
                .fold(f64::NEG_INFINITY, f64::max),
        )
    }

    /// 軌跡を CSV として書く。
    ///
    /// # Errors
    ///
    /// 書き込みに失敗した場合。
    pub fn write_csv<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writeln!(
            writer,
            "time_s,phase,latitude_deg,longitude_deg,altitude_m,ground_elevation_m,agl_m,\
             wheel_clearance_m,roll_deg,pitch_deg,heading_deg,airspeed_ms,ground_speed_ms,\
             vertical_speed_ms,aileron,elevator,rudder,throttle,flaps,brakes,terrain"
        )?;
        for sample in &self.samples {
            writeln!(
                writer,
                "{:.3},{},{:.7},{:.7},{:.2},{:.2},{:.2},{:.3},{:.2},{:.2},{:.2},\
                 {:.2},{:.2},{:.2},{:.4},{:.4},{:.4},{:.4},{:.2},{:.2},{}",
                sample.time.get(),
                sample.phase.name(),
                sample.position.latitude_degrees(),
                sample.position.longitude_degrees(),
                sample.position.altitude.get(),
                sample.ground_elevation.get(),
                sample.agl.get(),
                sample.wheel_clearance.get(),
                sample.attitude.roll.to_degrees().get(),
                sample.attitude.pitch.to_degrees().get(),
                sample.attitude.yaw.wrap_positive().to_degrees().get(),
                sample.airspeed.get(),
                sample.ground_speed.get(),
                sample.vertical_speed.get(),
                sample.controls.aileron(),
                sample.controls.elevator(),
                sample.controls.rudder(),
                sample.controls.throttle(),
                sample.controls.flaps(),
                sample.controls.brakes(),
                u8::from(sample.terrain_available),
            )?;
        }
        Ok(())
    }
}

/// 実行の設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulationOptions {
    /// 物理ステップ幅。ADR-0004 に従い固定。
    pub dt: Seconds,
    /// 打ち切り時間。
    pub max_duration: Seconds,
    /// 軌跡を記録する間隔。
    pub sample_interval: Seconds,
}

impl Default for SimulationOptions {
    fn default() -> Self {
        Self {
            dt: RECOMMENDED_FIXED_DT,
            max_duration: Seconds(600.0),
            sample_interval: Seconds(0.5),
        }
    }
}

/// 脚を伸ばし切った状態での重心の対地高度。
#[must_use]
pub fn gear_height(config: &AircraftConfig) -> Meters {
    let lowest = config
        .landing_gear
        .legs()
        .iter()
        .map(|leg| leg.contact_point().as_vec().z)
        .fold(f64::NEG_INFINITY, f64::max);
    Meters(if lowest.is_finite() {
        lowest.max(0.0)
    } else {
        0.0
    })
}

/// 滑走路上に静止した初期状態を作る。
///
/// 脚を伸ばし切った高さに置く。自重で数センチ沈んで落ち着くが、
/// 静的釣り合いを呼び出し側で解くより素直で、脚の内部モデルに依存しない。
#[must_use]
pub fn parked_state(
    config: &AircraftConfig,
    position: Geodetic,
    ground_elevation: Meters,
    heading: Radians,
) -> RigidBodyState {
    RigidBodyState::from_geodetic(
        Geodetic::new(
            position.latitude,
            position.longitude,
            Meters(ground_elevation.get() + gear_height(config).get()),
        ),
        Attitude::new(Radians::ZERO, Radians::ZERO, heading),
        Ned::new(0.0, 0.0, 0.0),
    )
}

/// 地形の上を飛ばして軌跡を返す。
///
/// # Panics
///
/// `options` の時間が正でない場合。
pub fn fly<S: TileSource>(
    config: &AircraftConfig,
    plan: &CircuitPlan,
    start: Geodetic,
    terrain: &mut Terrain<S>,
    sampler: &GroundSampler,
    options: &SimulationOptions,
) -> Trajectory {
    assert!(
        options.dt.get() > 0.0 && options.max_duration.get() > 0.0,
        "dt and max_duration must be positive"
    );

    let director = FlightDirector::default();
    let gear = gear_height(config).get();

    let initial_ground = sampler.sample(terrain, start);
    let mut dynamics = FlightDynamics::new(
        config.clone(),
        parked_state(config, start, initial_ground.elevation, plan.runway_heading),
    );

    let mut samples = Vec::new();
    let mut phase = Phase::TakeoffRoll;
    let mut phase_entered_at = 0.0_f64;
    let mut time = 0.0_f64;
    let mut next_sample_at = 0.0_f64;
    let mut steps_without_terrain = 0_u64;
    let mut diverged = false;

    while time < options.max_duration.get() && phase != Phase::Complete {
        let state = *dynamics.state();
        if !state.is_finite() {
            diverged = true;
            break;
        }

        let ground = sampler.sample(terrain, state.geodetic());
        if !ground.from_terrain {
            steps_without_terrain += 1;
        }

        let snapshot = observe(&state, &ground, gear);
        let targets = plan.targets(phase);
        let controls = director.control(&state, snapshot.agl, targets);

        if time >= next_sample_at {
            samples.push(record(time, phase, &state, &ground, &snapshot, controls));
            next_sample_at += options.sample_interval.get();
        }

        // 接地平面は 1 ステップの間固定される（ADR-0004）。
        let environment = Environment::still_air().with_ground_plane(
            ground.reference,
            ground.elevation,
            ground.slope,
        );
        dynamics.step(options.dt, controls, &environment);
        time += options.dt.get();

        let next = plan.next_phase(phase, time - phase_entered_at, &snapshot);
        if next != phase {
            phase = next;
            phase_entered_at = time;
        }
    }

    // 最終状態も必ず 1 点残す。時間切れの原因を追えるようにするため。
    let state = *dynamics.state();
    if state.is_finite() {
        let ground = sampler.sample(terrain, state.geodetic());
        let snapshot = observe(&state, &ground, gear);
        let controls = director.control(&state, snapshot.agl, plan.targets(phase));
        samples.push(record(time, phase, &state, &ground, &snapshot, controls));
    } else {
        diverged = true;
    }

    Trajectory {
        samples,
        final_phase: phase,
        duration: Seconds(time),
        diverged,
        steps_without_terrain,
    }
}

fn observe(state: &RigidBodyState, ground: &GroundPlane, gear: f64) -> Snapshot {
    let agl = state.altitude().get() - ground.elevation.get();
    Snapshot {
        airspeed: MetersPerSecond(state.body_velocity().length()),
        agl: Meters(agl),
        wheel_clearance: Meters(agl - gear),
        heading: state.attitude().yaw,
    }
}

fn record(
    time: f64,
    phase: Phase,
    state: &RigidBodyState,
    ground: &GroundPlane,
    snapshot: &Snapshot,
    controls: ControlInputs,
) -> TrajectorySample {
    TrajectorySample {
        time: Seconds(time),
        phase,
        position: state.geodetic(),
        ground_elevation: ground.elevation,
        agl: snapshot.agl,
        wheel_clearance: snapshot.wheel_clearance,
        attitude: state.attitude(),
        airspeed: snapshot.airspeed,
        ground_speed: state.ground_speed(),
        vertical_speed: state.vertical_speed(),
        controls,
        terrain_available: ground.from_terrain,
    }
}
