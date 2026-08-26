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
    AircraftConfig, ControlInputs, Environment, FlightDynamics, GroundSlope, RECOMMENDED_FIXED_DT,
    RigidBodyState,
};
use flightsim_world::{Runway, Terrain, TileSource};
use glam::{DMat3, DQuat, DVec3};

/// 車輪がこれ以上地面から離れていれば、確実に空中にいるとみなす高さ。
///
/// 脚の最大ストロークが 0.25 m なので、それを明確に上回る値にしてある。
pub(crate) const AIRBORNE_CLEARANCE: Meters = Meters(0.5);

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
    /// 精密進入に使う滑走路。`None` なら従来どおり方位だけで飛ぶ。
    pub runway: Option<Runway>,
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
    /// 精密進入で向かう最終進入フィックスの、末端手前からの距離。
    pub final_approach_distance: Meters,
    /// 最終進入フィックスを捕捉したとみなす半径。
    pub approach_capture_radius: Meters,
    /// 中心線捕捉で前方を見る距離。短すぎると蛇行し、長すぎると収束が遅い。
    pub guidance_lookahead: Meters,
    /// 接地点として狙う、進入端からの距離。
    pub touchdown_aim: Meters,
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
            runway: None,
            runway_heading: Radians::ZERO,
            rotate_speed: MetersPerSecond(30.0),
            climb_pitch: Radians(8.0_f64.to_radians()),
            pattern_altitude_agl: Meters(300.0),
            cruise_speed: MetersPerSecond(50.0),
            cruise_duration: Seconds(40.0),
            outbound_heading: Radians(90.0_f64.to_radians()),
            final_approach_distance: Meters(6_000.0),
            approach_capture_radius: Meters(250.0),
            guidance_lookahead: Meters(70.0),
            touchdown_aim: Meters(300.0),
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
    /// 実滑走路へ戻る精密な場周計画を作る。
    #[must_use]
    pub fn for_runway(runway: Runway) -> Self {
        Self {
            runway: Some(runway),
            runway_heading: runway.heading,
            // 左場周。精密誘導では最終進入フィックスへ向かうまでの初期目標として使う。
            outbound_heading: Radians(runway.heading.get() - core::f64::consts::FRAC_PI_2)
                .wrap_positive(),
            // 35 m/s で標準 3° の進入角に相当する降下率。
            approach_descent: MetersPerSecond(1.83),
            ..Self::default()
        }
    }

    /// フェーズに対応するディレクタへの指示。
    #[must_use]
    fn targets(&self, phase: Phase, position: Geodetic) -> DirectorTargets {
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
                heading: self.runway.map_or(self.outbound_heading, |runway| {
                    self.heading_to_final_fix(runway, position)
                }),
                ..base
            },
            Phase::Approach => DirectorTargets {
                vertical: VerticalTarget::DescentRate(self.approach_descent),
                heading: self.runway.map_or(self.outbound_heading, |runway| {
                    self.centerline_heading(runway, position)
                }),
                airspeed: self.approach_speed,
                flaps: self.approach_flaps,
                ..base
            },
            // フレアはスロットルを絞り、低い沈下率を保つ。
            Phase::Flare => DirectorTargets {
                vertical: VerticalTarget::DescentRate(self.flare_descent),
                heading: self.runway.map_or(self.outbound_heading, |runway| {
                    self.centerline_heading(runway, position)
                }),
                airspeed: self.approach_speed,
                flaps: self.approach_flaps,
                throttle_override: Some(0.0),
                ..base
            },
            Phase::Rollout => DirectorTargets {
                heading: self
                    .runway
                    .map_or(self.outbound_heading, |runway| runway.heading),
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

    /// 現在位置から最終進入フィックスへ向く真方位。
    fn heading_to_final_fix(&self, runway: Runway, position: Geodetic) -> Radians {
        let distance = finite_positive(self.final_approach_distance, Meters(6_000.0));
        heading_to_runway_point(&runway, position, Meters(-distance.get()), Meters::ZERO)
    }

    /// 前方注視点を使って滑走路中心線を捕捉する真方位。
    fn centerline_heading(&self, runway: Runway, position: Geodetic) -> Radians {
        let offsets = runway.offsets(position);
        if !offsets.is_finite() {
            return runway.heading.wrap_positive();
        }

        let lookahead = finite_positive(self.guidance_lookahead, Meters(70.0));
        let touchdown = finite_non_negative(self.touchdown_aim, Meters(300.0));
        // 現在位置より必ず前に注視点を置く。末端手前では接地点までを見て、
        // 滑走路上へ入った後は機体と一緒に前へ送るため、中心線を保ち続ける。
        let target_longitudinal = Meters(
            touchdown
                .get()
                .max(offsets.longitudinal.get() + lookahead.get()),
        );
        heading_to_runway_point(&runway, position, target_longitudinal, Meters::ZERO)
    }

    fn final_fix_distance(&self, runway: Runway, position: Geodetic) -> Meters {
        let offsets = runway.offsets(position);
        if !offsets.is_finite() {
            return Meters(f64::INFINITY);
        }
        let distance = finite_positive(self.final_approach_distance, Meters(6_000.0));
        Meters(
            ((offsets.longitudinal.get() + distance.get()).powi(2) + offsets.lateral.get().powi(2))
                .sqrt(),
        )
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
            Phase::Turn => {
                if let Some(runway) = self.runway {
                    let capture = finite_positive(self.approach_capture_radius, Meters(250.0));
                    // 出発後は滑走路の反対側からフィックスに入るため、
                    // 捕捉時の機首はまだ滑走路方位と一致しない。捕捉後に
                    // 中心線の前方注視点へ旋回させる。
                    if self.final_fix_distance(runway, sample.position).get() <= capture.get() {
                        Phase::Approach
                    } else {
                        Phase::Turn
                    }
                } else if sample
                    .heading
                    .shortest_difference_to(self.outbound_heading)
                    .get()
                    .abs()
                    < 5.0_f64.to_radians()
                {
                    Phase::Approach
                } else {
                    Phase::Turn
                }
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
    position: Geodetic,
    airspeed: MetersPerSecond,
    agl: Meters,
    wheel_clearance: Meters,
    heading: Radians,
}

fn finite_positive(value: Meters, fallback: Meters) -> Meters {
    if value.get().is_finite() && value.get() > 0.0 {
        value
    } else {
        fallback
    }
}

fn finite_non_negative(value: Meters, fallback: Meters) -> Meters {
    if value.get().is_finite() && value.get() >= 0.0 {
        value
    } else {
        fallback
    }
}

/// 滑走路ローカル座標上の点へ向く真方位。
///
/// 測地変換は [`Runway::offsets`] に閉じ込め、ここでは航法上の二次元ベクトルだけを扱う。
fn heading_to_runway_point(
    runway: &Runway,
    position: Geodetic,
    target_longitudinal: Meters,
    target_lateral: Meters,
) -> Radians {
    let offsets = runway.offsets(position);
    let forward = target_longitudinal.get() - offsets.longitudinal.get();
    let right = target_lateral.get() - offsets.lateral.get();
    if !offsets.is_finite()
        || !forward.is_finite()
        || !right.is_finite()
        || (forward.abs() < 1.0e-9 && right.abs() < 1.0e-9)
    {
        return runway.heading.wrap_positive();
    }
    Radians(runway.heading.get() + right.atan2(forward)).wrap_positive()
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
    /// 定常風。既定は無風。
    ///
    /// **自動操縦は風を知らない。** 方位を保つ制御しか持たないので、
    /// 横風では風下へ流される。それが正しい挙動で、
    /// 偏流修正はプレイヤー（または将来の誘導）の仕事。
    pub wind: crate::simulation::Wind,
}

impl Default for SimulationOptions {
    fn default() -> Self {
        Self {
            wind: crate::simulation::Wind::CALM,
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
///
/// # 姿勢を斜面に合わせる
///
/// 傾斜地では、水平姿勢のどの置き方にも欠陥がある:
///
/// - 「基準点の標高 + 脚の高さ」に置く → 上り側の車輪が最初から地面に入る
///   （15° 斜面で前脚 0.43 m）。めり込みが脚のばねに偽のエネルギーとして
///   蓄えられ、機体を背面まで一回転させた
/// - どの脚も入らない高さへ持ち上げる → 反対側の脚が浮き（25° 斜面で
///   約 1.5 m）、**落下エネルギー約 7.6 kJ が転倒障壁 2.9 kJ を上回って
///   やはり裏返る**。実際に CI がこれを捕まえた
///
/// 実機が斜面に駐機したときと同じく、**機体を斜面に沿って傾け、全脚を
/// 同時に接地させる**。落下もめり込みも起きない。
#[must_use]
pub fn parked_state(
    config: &AircraftConfig,
    position: Geodetic,
    ground_elevation: Meters,
    slope: GroundSlope,
    heading: Radians,
) -> RigidBodyState {
    let attitude = attitude_on_slope(slope, heading);

    // 傾けた姿勢で各脚の接地点がどこへ来るかを見て、
    // いちばん厳しい脚がちょうど地面に触れる高さへ重心を置く。
    // 姿勢が斜面に沿っていれば、全脚の値はほぼ一致する。
    let rotation = attitude.to_quaternion();
    let clearance_needed = config
        .landing_gear
        .legs()
        .iter()
        .map(|leg| {
            let ned = rotation * leg.contact_point().as_vec();
            let ground_rise = slope.north() * ned.x + slope.east() * ned.y;
            // z は下向き正。脚の下端 + その位置の地面の持ち上がり。
            ned.z + ground_rise
        })
        .fold(f64::NEG_INFINITY, f64::max);

    let clearance_needed = if clearance_needed.is_finite() {
        clearance_needed.max(0.0)
    } else {
        gear_height(config).get()
    };

    RigidBodyState::from_geodetic(
        Geodetic::new(
            position.latitude,
            position.longitude,
            Meters(ground_elevation.get() + clearance_needed),
        ),
        attitude,
        Ned::new(0.0, 0.0, 0.0),
    )
}

/// 指定方位で斜面に沿って立つ姿勢。
///
/// 機体の下方向（体軸 Z）を斜面の法線に合わせ、機首は方位の水平方向を
/// 斜面に投影した向きにする。NED（x=北, y=東, z=下）は右手系なので
/// `y = z × x` で右翼方向が決まる。
fn attitude_on_slope(slope: GroundSlope, heading: Radians) -> Attitude {
    // 地面 z_down = -(sn·北 + se·東) の下向き法線。勾配が非有限なら水平。
    let (north, east) = if slope.is_finite() {
        (slope.north(), slope.east())
    } else {
        (0.0, 0.0)
    };
    let body_down = DVec3::new(north, east, 1.0).normalize();

    let (sin, cos) = heading.get().sin_cos();
    let level_forward = DVec3::new(cos, sin, 0.0);
    // 方位の水平方向を斜面へ投影。法線が水平に近い勾配は
    // GroundSampler 側で上限が掛かるため、ここでは縮退しない。
    let forward = (level_forward - body_down * level_forward.dot(body_down)).normalize();
    let right = body_down.cross(forward);

    // 列 = 機体軸を NED で表したもの。体軸 → NED の回転。
    let rotation = DQuat::from_mat3(&DMat3::from_cols(forward, right, body_down)).normalize();
    Attitude::from_quaternion(rotation)
}

/// 最終進入の途中から始める状態を作る。
///
/// # なぜ要るのか
///
/// **着陸だけを練習したいのに、毎回場周を一周させるのは辛い。**
/// ゲームとしての核が着陸の腕なら、そこへすぐ入れる道が要る。
///
/// 滑走路の末端から `distance` 手前、標準的な 3 度の進入角に乗った高さで、
/// 中心線に正対して進入速度で降下している状態を返す。
///
/// # 引数
///
/// * `distance` — 末端までの距離。負や非有限は 1 海里へ丸める
/// * `glideslope` — 進入角。実機の標準は 3 度
/// * `speed` — 進入対気速度。無風なら対地速度と同じ
#[must_use]
pub fn approach_state(
    runway: &Runway,
    distance: Meters,
    glideslope: Radians,
    speed: MetersPerSecond,
) -> RigidBodyState {
    // 手前 = 進行方向の負側。
    let distance = if distance.get().is_finite() && distance.get() > 0.0 {
        distance.get()
    } else {
        1852.0
    };
    let glideslope = if glideslope.get().is_finite() {
        glideslope.get().clamp(0.0, core::f64::consts::FRAC_PI_4)
    } else {
        3.0_f64.to_radians()
    };
    let speed = if speed.get().is_finite() && speed.get() > 0.0 {
        speed.get()
    } else {
        35.0
    };

    let ground_point = runway.point_at(Meters(-distance), Meters::ZERO);
    // 3 度の進入角なら、末端から 1 海里で約 97 m（320 ft）上。
    let height = distance * glideslope.tan();
    let position = Geodetic::new(
        ground_point.latitude,
        ground_point.longitude,
        Meters(runway.elevation.get() + height),
    );

    // 速度は機首方位へ、進入角ぶん下向き。
    let (sin_heading, cos_heading) = runway.heading.get().sin_cos();
    let horizontal = speed * glideslope.cos();
    let velocity = Ned::new(
        horizontal * cos_heading,
        horizontal * sin_heading,
        // NED の down は正。降下しているので正。
        speed * glideslope.sin(),
    );

    // 姿勢は水平・中心線に正対。**ピッチを進入角に合わせない。**
    // 実機の進入は機首上げで、経路角とピッチは一致しない（迎角のぶん違う）。
    // ここで経路角をそのままピッチにすると、機首下げで進入することになる。
    RigidBodyState::from_geodetic(
        position,
        Attitude::new(Radians::ZERO, Radians::ZERO, runway.heading),
        velocity,
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
        parked_state(
            config,
            start,
            initial_ground.elevation,
            initial_ground.slope,
            plan.runway_heading,
        ),
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

        let snapshot = observe(&state, &ground, gear, options.wind);
        let targets = plan.targets(phase, state.geodetic());
        let controls = director.control(&state, snapshot.agl, targets);

        if time >= next_sample_at {
            samples.push(record(time, phase, &state, &ground, &snapshot, controls));
            next_sample_at += options.sample_interval.get();
        }

        // 接地平面は 1 ステップの間固定される（ADR-0004）。
        let environment = Environment::with_wind_ned(
            flightsim_fdm::Atmosphere::standard(),
            state.geodetic(),
            options.wind.to_ned(),
        )
        .with_ground_plane(ground.reference, ground.elevation, ground.slope);
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
        let snapshot = observe(&state, &ground, gear, options.wind);
        let controls =
            director.control(&state, snapshot.agl, plan.targets(phase, state.geodetic()));
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

/// 現在の状態から、自動操縦が見る観測量を作る。
///
/// **対気速度は風を差し引いて計算する。** 対地速度で回転や進入速度を
/// 判断すると、向かい風の中で必要以上に加速してから浮くことになる。
fn observe(
    state: &RigidBodyState,
    ground: &GroundPlane,
    gear: f64,
    wind: crate::simulation::Wind,
) -> Snapshot {
    let agl = state.altitude().get() - ground.elevation.get();
    let wind_ecef =
        flightsim_core::LocalFrame::new(state.geodetic()).ned_to_ecef_vector(wind.to_ned());
    Snapshot {
        position: state.geodetic(),
        airspeed: MetersPerSecond((state.velocity - wind_ecef).length()),
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

#[cfg(test)]
mod guidance_tests {
    use super::*;
    use flightsim_core::Degrees;

    fn angular_error(actual: Radians, expected: Radians) -> f64 {
        actual.shortest_difference_to(expected).get().abs()
    }

    #[test]
    fn a_point_on_the_extended_centerline_commands_runway_heading() {
        let runway = Runway::synthetic();
        let position = runway.point_at(Meters(-3_000.0), Meters::ZERO);
        let heading = heading_to_runway_point(&runway, position, Meters(300.0), Meters::ZERO);
        assert!(angular_error(heading, runway.heading) < 1.0e-9);
    }

    #[test]
    fn an_aircraft_right_of_centerline_is_commanded_left() {
        let runway = Runway::synthetic();
        let position = runway.point_at(Meters(-1_000.0), Meters(200.0));
        let heading = heading_to_runway_point(&runway, position, Meters(300.0), Meters::ZERO);
        let correction = runway.heading.shortest_difference_to(heading).get();
        assert!(
            correction < 0.0,
            "correction was {} deg",
            correction.to_degrees()
        );
        // 外部の平面幾何: atan2(-200, 1300) = -8.746°。
        assert!((correction.to_degrees() + 8.746).abs() < 0.01);
    }

    #[test]
    fn an_aircraft_left_of_centerline_is_commanded_right() {
        let runway = Runway::synthetic();
        let position = runway.point_at(Meters(-1_000.0), Meters(-200.0));
        let heading = heading_to_runway_point(&runway, position, Meters(300.0), Meters::ZERO);
        assert!(runway.heading.shortest_difference_to(heading).get() > 0.0);
    }

    #[test]
    fn guidance_wraps_cleanly_across_north() {
        let runway = Runway::from_degrees(35.0, 139.0, 350.0, 2_500.0, 45.0, 0.0);
        let position = runway.point_at(Meters(-1_000.0), Meters(-500.0));
        let heading = heading_to_runway_point(&runway, position, Meters(300.0), Meters::ZERO);
        assert!((0.0..core::f64::consts::TAU).contains(&heading.get()));
        assert!(runway.heading.shortest_difference_to(heading).get() > 0.0);
    }

    #[test]
    fn non_finite_guidance_falls_back_to_runway_heading() {
        let runway = Runway::synthetic();
        let invalid = Geodetic::new(Radians(f64::NAN), Radians(f64::INFINITY), Meters(f64::NAN));
        let heading = heading_to_runway_point(&runway, invalid, Meters(f64::NAN), Meters::ZERO);
        assert_eq!(heading, runway.heading.wrap_positive());
    }

    #[test]
    fn runway_plan_captures_the_final_fix_before_descending() {
        let runway = Runway::synthetic();
        let plan = CircuitPlan::for_runway(runway);
        let position = runway.point_at(Meters(-plan.final_approach_distance.get()), Meters::ZERO);
        let sample = Snapshot {
            position,
            airspeed: MetersPerSecond(35.0),
            agl: Meters(100.0),
            wheel_clearance: Meters(99.0),
            heading: runway.heading,
        };
        assert_eq!(plan.next_phase(Phase::Turn, 0.0, &sample), Phase::Approach);

        let far = Snapshot {
            position: runway.point_at(Meters(-9_000.0), Meters::ZERO),
            ..sample
        };
        assert_eq!(plan.next_phase(Phase::Turn, 0.0, &far), Phase::Turn);
    }

    #[test]
    fn centerline_lookahead_uses_finite_defaults_for_bad_configuration() {
        let runway = Runway::synthetic();
        let plan = CircuitPlan {
            runway: Some(runway),
            guidance_lookahead: Meters(f64::NAN),
            touchdown_aim: Meters(f64::NEG_INFINITY),
            ..CircuitPlan::for_runway(runway)
        };
        let position = runway.point_at(Meters(-1_000.0), Meters(100.0));
        let heading = plan.centerline_heading(runway, position);
        assert!(heading.is_finite());
        assert!(runway.heading.shortest_difference_to(heading).get() < 0.0);
    }

    #[test]
    fn constructor_uses_a_left_hand_pattern_and_normalises_it() {
        let runway = Runway::from_degrees(0.0, 0.0, 10.0, 2_000.0, 45.0, 0.0);
        let plan = CircuitPlan::for_runway(runway);
        assert_eq!(plan.runway, Some(runway));
        assert!(angular_error(plan.outbound_heading, Degrees(280.0).to_radians()) < 1.0e-12);
    }
}
