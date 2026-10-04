//! 飛行の記録と再生。
//!
//! # 何を記録するのか
//!
//! **操縦入力とフレーム時間だけ**を記録し、再生時は同じ物理を回し直す。
//! 姿勢や位置を毎フレーム記録して再生時に流し込む方式は取らない。
//! 前者なら再生中に接地判定も計器も評価も本物と同じ経路を通るが、後者だと
//! 「絵は動くが中身は空」になり、リプレイで何かを検証することができない。
//!
//! 成り立つのは FDM が決定論的だから（[ADR-0004]）。壁時計・乱数・
//! グローバル可変状態を参照しないので、同じ入力列は同じ軌跡になる。
//!
//! ## それでもずれる場合がある
//!
//! 決定論が保証するのは**同じビルド・同じ環境**での一致だけ。
//!
//! - Aircraft parameters or the FDM model revision can change the trajectory.
//!   v1/v2 fingerprints omit `aero.yaw_rate_p`: a match is only legacy partial
//!   compatibility. [`identity`] provides complete aircraft identity separately;
//!   it does not change old bytes or supply missing evidence for old recordings.
//! - **地形が違えば接地が変わる** → 地形は指紋を取れない（タイルは実行時に
//!   ストリーミングされ、どれが読まれたかは軌跡に依存する）。代わりに
//!   [`Keyframe`] を一定間隔で埋め込み、再生側が実際にずれたことを**検出**する
//! - 浮動小数の丸めは同じ命令列なら同じ。異なる CPU / 最適化での一致は検証していない
//!
//! 「ずれない」とは書かない。**ずれたら分かる**ようにしてある。
//!
//! # 何ができるか
//!
//! | 操作 | 方法 |
//! |---|---|
//! | 一時停止・再開 | [`Player::set_paused`] |
//! | 速度変更 | [`Player::set_speed`]（0.1〜8 倍） |
//! | 前進シーク | 目標まで [`Player::next_due`] を空回しする |
//! | Exact backward seek | [`Player::seek`] to frame zero, restart the simulation, then replay to the target |
//!
//! State-only keyframes are drift checkpoints, not complete simulation snapshots.
//! Exact seeking restarts at frame zero: the accumulator, turbulence clock, contact
//! history and flight log cannot be restored by assigning only a rigid-body state.
//!
//! # 使い方
//!
//! ```
//! use flightsim_core::Seconds;
//! use flightsim_fdm::ControlInputs;
//! use flightsim_sim::replay::{Conditions, Player, Recorder};
//!
//! let mut recorder = Recorder::new(Conditions::default());
//! for _ in 0..10 {
//!     recorder.record(Seconds(1.0 / 60.0), ControlInputs::neutral().with_throttle(0.5), None);
//! }
//! let recording = recorder.finish();
//!
//! let mut bytes = Vec::new();
//! recording.write_to(&mut bytes).expect("writing to a Vec cannot fail");
//! let restored = Recording::read_from(&mut &bytes[..]).expect("round trip");
//! assert_eq!(restored.frames().len(), 10);
//!
//! let mut player = Player::new(restored);
//! player.accumulate(Seconds(1.0));
//! assert!(player.next_due().is_some());
//! # use flightsim_sim::replay::Recording;
//! ```
//!
//! [ADR-0004]: https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0004-simulation-loop.md

use std::io::{Read, Write};

use flightsim_core::{Geodetic, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, FDM_MODEL_REVISION, RigidBodyState, Turbulence,
};
use flightsim_world::global::GLOBAL_TERRAIN_FINGERPRINT;
use flightsim_world::{ClimateDate, GLOBAL_CLIMATE_FINGERPRINT};
use glam::{DQuat, DVec3};

use crate::simulation::Wind;

mod current;
pub mod identity;
mod player;
use player::PlaybackCursor;
pub use player::ReplayFilePlayer;

pub use current::{
    CURRENT_FORMAT_VERSION, CurrentConditions, CurrentRecorder, CurrentRecording,
    EnvironmentConditions, MAX_CONDITIONS_BYTES, MAX_WEATHER_BYTES, ReplayFile,
};

use identity::{AircraftCompatibility, RecordedAircraftIdentity};

/// ファイル先頭の識別子。
pub const MAGIC: [u8; 8] = *b"FSREPLAY";

/// Historical maximum used by the legacy [`Recording`] API. Kept at 2 for
/// source compatibility; [`ReplayFile`] dispatches through [`CURRENT_FORMAT_VERSION`].
pub const FORMAT_VERSION: u16 = WORLD_FORMAT_VERSION;

/// Version 2, with the fixed world/climate extension. Never use the newest
/// supported version as a test for whether this legacy extension is present.
pub const WORLD_FORMAT_VERSION: u16 = 2;

/// Original format. The writer retains its exact layout when world/climate
/// settings are disabled; old recordings always restore ISA/legacy terrain.
pub const LEGACY_FORMAT_VERSION: u16 = 1;

/// キーフレームを置く間隔（記録フレーム数）。
///
/// One second for new 120 Hz fixed-step recordings (two seconds for old 60 fps
/// render-frame recordings). These state-only checkpoints detect drift; exact
/// seeking must replay from frame zero to restore clocks, contacts and flight logs.
pub const KEYFRAME_INTERVAL: u32 = 120;

/// 読み込みを受け付けるフレーム数の上限。
///
/// About 2 h 18 m 53 s at 120 Hz (4.6 h for old 60 fps recordings). The frame
/// payload is at most 56 MB, plus bounded keyframes and metadata. This resource
/// limit is unchanged by fixed-step recording; it is not a duration guarantee.
/// **Never trust a corrupt count field to determine an unbounded allocation.**
pub const MAX_FRAMES: u32 = 1_000_000;

/// 機体名として受け付けるバイト数の上限。
pub const MAX_NAME_BYTES: u32 = 256;

/// Largest supported visual-clock Julian date: UTC 2147483647-12-31 midnight.
///
/// The visual calendar uses an `i32` year. Reserving its final day provides
/// rounding headroom, and this bound also keeps calendar and solar arithmetic
/// finite. It is a representation limit, not a claim of astronomical accuracy
/// at remote dates. Epoch zero in [`Conditions`] remains "not recorded".
/// A caller resolving that sentinel must check its chosen origin plus the
/// scaled recording duration against this same bound.
pub const MAX_VISUAL_EPOCH: f64 = 784_354_017_363.5;

/// 再生速度の下限。0 は「停止」であって速度ではないので [`Player::set_paused`] を使う。
pub const MIN_SPEED: f64 = 0.1;

/// 再生速度の上限。
///
/// 物理は記録どおりのフレームを順に流すので、速くするほど 1 描画フレームで
/// 回す物理ステップが増える。**上げすぎると再生の方が本編より重くなる。**
pub const MAX_SPEED: f64 = 8.0;

/// 1 フレームぶんの記録（フレーム時間 + 操縦入力 6 つ）。
const FRAME_BYTES: usize = 8 * 7;

/// キーフレーム 1 つぶんのバイト数（フレーム番号 + 剛体状態 13 要素）。
const KEYFRAME_BYTES: usize = 4 + 8 * 13;

/// 記録した 1 フレーム。
///
/// New interactive recordings store one fixed physics step and its controls per
/// record, using [`crate::Simulation::advance_with_controls`]. Existing files may
/// contain render-frame durations (including zero); their bytes and durations are
/// retained exactly. In both cases, replay passes the stored duration and controls
/// to [`crate::Simulation::advance`] without relabeling or resampling the records.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// このフレームで進めた時間。ファイル境界では有限・非負（0 も有効）。
    pub frame_time: Seconds,
    /// このフレームで与えた操縦入力。
    pub controls: ControlInputs,
}

/// 一定間隔で埋め込む状態の写し。
///
/// A checkpoint for detecting replay drift. Frame zero is the exact restart point;
/// later states alone do not restore the simulation clock, contacts or flight log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    /// このキーフレームが対応するフレーム番号（このフレームを進める**前**の状態）。
    pub frame: u32,
    /// そのときの剛体状態。有限なベクトルと単位 quaternion を保存する。
    pub state: RigidBodyState,
}

/// 再生に必要な初期条件。
///
/// **ここが違えば同じ軌跡にならない。** だから全部記録する。
#[derive(Debug, Clone, PartialEq)]
pub struct Conditions {
    /// 機体名。人が読むためのもの。一致判定には使わない。
    pub aircraft_name: String,
    /// Frozen v1/v2 partial fingerprint, including the FDM model revision but
    /// omitting `aero.yaw_rate_p`. Never reinterpret this field as complete identity.
    pub aircraft_fingerprint: u64,
    /// 開始位置。
    pub start: Geodetic,
    /// 開始方位。
    pub heading: Radians,
    /// 定常風。
    pub wind: Wind,
    /// 乱流。
    pub turbulence: Turbulence,
    /// 開始時刻の UTC ユリウス日。太陽位置と灯火の再現に要る。
    ///
    /// **「時刻」ではなく暦上の一点**を持つ。時分だけだと日付が落ち、
    /// 夏至と冬至で太陽高度が変わってしまう。
    /// 0 は「記録側が時刻を持っていない」印で、再生側は既定の時刻を使う。
    /// ファイルでは有限な `0..=MAX_VISUAL_EPOCH` に限る。
    pub start_epoch: f64,
    /// 記録時の時間加速率。ファイルでは有限・非負に限る。
    pub time_rate: f64,
    /// Use the bundled global terrain baseline. False preserves legacy terrain
    /// behavior; explicitly supplied regional tiles remain caller-owned.
    pub world_terrain: bool,
    /// Identity of the enabled global terrain payload, otherwise zero.
    pub terrain_fingerprint: u64,
    /// Fixed seasonal climatology for this flight. None preserves ISA physics.
    pub climate_date: Option<ClimateDate>,
    /// Identity of the enabled climate payload, otherwise zero.
    pub climate_fingerprint: u64,
}

impl Default for Conditions {
    fn default() -> Self {
        Self {
            aircraft_name: String::new(),
            aircraft_fingerprint: 0,
            start: Geodetic::from_degrees(0.0, 0.0, 0.0),
            heading: Radians(0.0),
            wind: Wind::CALM,
            turbulence: Turbulence::CALM,
            start_epoch: 0.0,
            time_rate: 1.0,
            world_terrain: false,
            terrain_fingerprint: 0,
            climate_date: None,
            climate_fingerprint: 0,
        }
    }
}

impl Conditions {
    /// 機体諸元から名前と指紋を埋める。
    #[must_use]
    pub fn with_aircraft(mut self, config: &AircraftConfig) -> Self {
        self.aircraft_name = config.name.clone();
        self.aircraft_fingerprint = aircraft_fingerprint(config);
        self
    }

    /// Describe the evidence actually stored by the unchanged v1/v2 formats.
    /// A name or current configuration cannot fill in their omitted coefficient.
    #[must_use]
    pub const fn aircraft_identity(&self) -> RecordedAircraftIdentity {
        RecordedAircraftIdentity::LegacyPartial {
            fingerprint: self.aircraft_fingerprint,
        }
    }

    /// Populate replay-safe identities for this build's bundled world data.
    /// This must be called before recording the first physics frame.
    #[must_use]
    pub fn with_world_climate(
        mut self,
        world_terrain: bool,
        climate_date: Option<ClimateDate>,
    ) -> Self {
        self.world_terrain = world_terrain;
        self.terrain_fingerprint = if world_terrain {
            GLOBAL_TERRAIN_FINGERPRINT
        } else {
            0
        };
        self.climate_date = climate_date;
        self.climate_fingerprint = if climate_date.is_some() {
            GLOBAL_CLIMATE_FINGERPRINT
        } else {
            0
        };
        self
    }
}

/// 機体諸元の指紋。
///
/// Frozen legacy algorithm: mixes most flight-affecting parameters and
/// [`FDM_MODEL_REVISION`], never the display name. It omits `aero.yaw_rate_p`.
/// A matching value is only [`AircraftCompatibility::LegacyPartialMatch`],
/// never proof of complete aircraft identity. Use [`identity::AircraftIdentity`]
/// for a new, complete identity contract; do not put it in the old u64 field.
/// Pre-revision alpha.21 identities are rejected without changing v1/v2 file
/// layouts, or rewriting old recordings as if they used the current model.
///
/// 完全なハッシュではなく、値が 1 つでも変われば高い確率で変わる程度のもの。
/// 目的は「気付かず違う機体で再生する」を防ぐことで、改竄検出ではない。
#[must_use]
pub fn aircraft_fingerprint(config: &AircraftConfig) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |value: f64| {
        // NaN の bit 表現は複数あるが、諸元に NaN が入っている時点で
        // 再生以前の問題なので、正規化はしない。
        for byte in value.to_bits().to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let mass = &config.mass_properties;
    mix(mass.mass().get());
    let inertia = mass.inertia();
    for column in inertia.to_cols_array() {
        mix(column);
    }
    mix(config.geometry.wing_area.get());
    mix(config.geometry.wing_span.get());
    mix(config.geometry.mean_chord.get());
    let aero = &config.aero;
    for value in [
        aero.lift_zero,
        aero.lift_alpha,
        aero.lift_flaps,
        aero.stall_angle.get(),
        aero.stall_blend_rate,
        aero.drag_min,
        aero.oswald_efficiency,
        aero.drag_flaps,
        aero.side_beta,
        aero.side_rudder,
        aero.roll_beta,
        aero.roll_rate_p,
        aero.roll_rate_r,
        aero.roll_aileron,
        aero.roll_rudder,
        aero.pitch_zero,
        aero.pitch_alpha,
        aero.pitch_rate_q,
        aero.pitch_elevator,
        aero.pitch_flaps,
        aero.yaw_beta,
        aero.yaw_rate_r,
        aero.yaw_rudder,
        aero.yaw_aileron,
    ] {
        mix(value);
    }
    let engine = &config.engine;
    mix(engine.max_shaft_power);
    mix(engine.propeller_efficiency);
    mix(engine.static_thrust.get());
    // 脚は接地の挙動を決める。**滑走と接地評価が変わるので外せない。**
    let gear = &config.landing_gear;
    mix(gear.rolling_friction_coefficient());
    mix(gear.braking_friction_coefficient());
    mix(gear.lateral_friction_coefficient());
    mix(gear.friction_transition_speed().get());
    for leg in gear.legs() {
        let point = leg.contact_point().as_vec();
        mix(point.x);
        mix(point.y);
        mix(point.z);
        mix(leg.spring_rate().get());
        mix(leg.damping_coefficient().get());
        mix(leg.max_stroke().get());
        mix(leg.bottom_stop_travel().get());
        mix(leg.max_recoil_speed().get());
    }
    // Domain-separated suffix: retain every config value above, then bind the
    // physics implementation even when its JSON parameters are unchanged.
    for byte in b"flightsim-fdm-model"
        .iter()
        .copied()
        .chain(FDM_MODEL_REVISION.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 記録一式。
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    conditions: Conditions,
    frames: Vec<Frame>,
    keyframes: Vec<Keyframe>,
}

impl Recording {
    /// 初期条件。
    #[must_use]
    pub const fn conditions(&self) -> &Conditions {
        &self.conditions
    }

    /// 記録したフレーム列。
    #[must_use]
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// 埋め込まれたキーフレーム。フレーム番号の昇順。
    #[must_use]
    pub fn keyframes(&self) -> &[Keyframe] {
        &self.keyframes
    }

    /// 記録された飛行時間の合計。
    ///
    /// **壁時計時間ではない。** 記録時に時間加速していれば実時間より長い。
    #[must_use]
    pub fn duration(&self) -> Seconds {
        Seconds(
            self.frames
                .iter()
                .fold(0.0, |elapsed, frame| elapsed + frame.frame_time.get()),
        )
    }

    /// `frame` 以下で最も後ろのキーフレーム。後退シークの開始点。
    #[must_use]
    pub fn keyframe_at_or_before(&self, frame: u32) -> Option<Keyframe> {
        match self.keyframes.binary_search_by_key(&frame, |key| key.frame) {
            Ok(index) => Some(self.keyframes[index]),
            Err(0) => None,
            Err(index) => Some(self.keyframes[index - 1]),
        }
    }

    /// このフレーム番号に検査点があれば返す。
    #[must_use]
    pub fn keyframe_exactly_at(&self, frame: u32) -> Option<Keyframe> {
        self.keyframes
            .binary_search_by_key(&frame, |key| key.frame)
            .ok()
            .map(|index| self.keyframes[index])
    }

    /// 再生中の状態が記録とどれだけ離れたか。検査点が無いフレームでは `None`。
    ///
    /// **距離が返るだけで、良し悪しは判断しない。** どこから「ずれた」と
    /// 呼ぶかは呼び出し側が決める（[`Player::DIVERGENCE_LIMIT`] が目安）。
    #[must_use]
    pub fn drift_at(&self, frame: u32, state: &RigidBodyState) -> Option<Meters> {
        self.keyframe_exactly_at(frame)
            .map(|key| Meters((state.position.0 - key.state.position.0).length()))
    }
}

/// 記録する側。
#[derive(Debug, Clone)]
pub struct Recorder {
    recording: Recording,
}

impl Recorder {
    /// 初期条件を決めて記録を始める。
    #[must_use]
    pub const fn new(conditions: Conditions) -> Self {
        Self {
            recording: Recording {
                conditions,
                frames: Vec::new(),
                keyframes: Vec::new(),
            },
        }
    }

    /// これまでに記録したフレーム数。
    #[must_use]
    pub fn frame_count(&self) -> u32 {
        // 上限で打ち切るので u32 に収まる。
        u32::try_from(self.recording.frames.len()).unwrap_or(u32::MAX)
    }

    /// 上限に達して、これ以上記録しないか。
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.recording.frames.len() >= MAX_FRAMES as usize
    }

    /// 1 フレーム記録する。
    ///
    /// `state` はこのフレームを進める**前**の状態。キーフレームの間隔に
    /// 当たったときだけ使われる。毎フレーム渡してよい（保持はしない）。
    ///
    /// **上限に達したら黙って捨てる。** ここでエラーを返しても呼び出し側は
    /// 飛行中に何もできない。[`Self::is_full`] で先に気付ける。
    /// 数値はここでは検証しない。不正な記録は [`Recording::write_to`] と
    /// [`Recording::check_reproducible_with`] がエラーにする。
    pub fn record(
        &mut self,
        frame_time: Seconds,
        controls: ControlInputs,
        state: Option<&RigidBodyState>,
    ) {
        if self.is_full() {
            return;
        }
        let index = self.frame_count();
        if index % KEYFRAME_INTERVAL == 0
            && let Some(state) = state
        {
            self.recording.keyframes.push(Keyframe {
                frame: index,
                state: *state,
            });
        }
        self.recording.frames.push(Frame {
            frame_time,
            controls,
        });
    }

    /// 記録を取り出す。
    #[must_use]
    pub fn finish(self) -> Recording {
        self.recording
    }

    /// 記録中の内容を覗く。
    #[must_use]
    pub const fn recording(&self) -> &Recording {
        &self.recording
    }
}

/// 後退シークの手順。
///
/// **そのまま飛ばせる状態ではない。** `state` から `replay_from` 番目の
/// フレームを `target` まで流し直して初めて目的の時点になる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeekPlan {
    /// 再計算の開始状態。
    pub state: RigidBodyState,
    /// 開始状態に対応するフレーム番号。
    pub replay_from: u32,
    /// 目的のフレーム番号。
    pub target: u32,
}

impl SeekPlan {
    /// 流し直すフレーム数。
    #[must_use]
    pub const fn frames_to_replay(self) -> u32 {
        self.target.saturating_sub(self.replay_from)
    }
}

/// 再生する側。
///
/// 自分では物理を回さない。**次に流すフレームを配るだけ**で、実際に進めるのは
/// 呼び出し側（[`crate::Simulation::advance`]）。`Simulation` は地形を持つので
/// 型引数が付き、ここに抱えると再生器まで地形の型に汚染される。
#[derive(Debug, Clone)]
pub struct Player {
    recording: Recording,
    playback: PlaybackCursor,
}

impl Player {
    /// これ以上離れたら「別の飛行になった」と見なす目安 `m`。
    ///
    /// 積分の丸めだけなら数分飛んでも 1 m には届かない。これを超えるのは
    /// **地形か機体か物理が違う**ということ。値は判断の目安であって、
    /// この型は超えても勝手に止めない。
    pub const DIVERGENCE_LIMIT: Meters = Meters(50.0);

    /// 記録を読み込んで先頭に置く。
    #[must_use]
    pub const fn new(recording: Recording) -> Self {
        Self {
            recording,
            playback: PlaybackCursor::new(),
        }
    }

    /// 再生元の記録。
    #[must_use]
    pub const fn recording(&self) -> &Recording {
        &self.recording
    }

    /// 次に流すフレーム番号。
    #[must_use]
    pub const fn cursor(&self) -> u32 {
        self.playback.cursor
    }

    /// 最後まで流し終えたか。
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.playback.cursor as usize >= self.recording.frames.len()
    }

    /// 一時停止しているか。
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.playback.paused
    }

    /// 一時停止・再開。
    ///
    /// 一時停止中は時間を溜めない。**溜めると再開の瞬間に早送りになる。**
    pub const fn set_paused(&mut self, paused: bool) {
        self.playback.set_paused(paused);
    }

    /// 再生速度。
    #[must_use]
    pub const fn speed(&self) -> f64 {
        self.playback.speed
    }

    /// 再生速度を変える。[`MIN_SPEED`]〜[`MAX_SPEED`] に丸める。
    ///
    /// NaN は 1 倍に倒す。`f64::clamp` は NaN をそのまま返すので、
    /// 素直に書くと速度が NaN になって再生が止まる。
    pub const fn set_speed(&mut self, speed: f64) {
        self.playback.set_speed(speed);
    }

    /// 描画フレームごとに 1 回呼び、進めてよい時間を足す。
    pub fn accumulate(&mut self, real_frame_time: Seconds) {
        self.playback.accumulate(real_frame_time);
    }

    /// 溜めた時間の範囲で次のフレームを 1 つ配る。無ければ `None`。
    ///
    /// 予算が尽きるまで繰り返し呼ぶ。**時間を足すのは
    /// [`Self::accumulate`] だけ**なので、何度呼んでも二重に進まない。
    pub fn next_due(&mut self) -> Option<Frame> {
        self.playback.next_due(self.recording.frames())
    }

    /// 一時停止に関係なく次のフレームを 1 つ取り出す。シークの空回しに使う。
    pub fn step_once(&mut self) -> Option<Frame> {
        self.playback.step_once(self.recording.frames())
    }

    /// 指定フレームへ移る。
    ///
    /// 前進なら [`SeekPlan::state`] は現在位置のキーフレームではなく直前の
    /// キーフレームになる。**呼び出し側は必ず [`SeekPlan`] のとおりに
    /// 流し直すこと。** カーソルだけ動かすと状態と番号が食い違う。
    ///
    /// キーフレームが 1 つも無い記録では `None`。その場合は最初から流し直す。
    pub fn seek(&mut self, frame: u32) -> Option<SeekPlan> {
        let target = frame.min(self.frame_count());
        let keyframe = self.recording.keyframe_at_or_before(target)?;
        self.playback.seek(keyframe.frame);
        Some(SeekPlan {
            state: keyframe.state,
            replay_from: keyframe.frame,
            target,
        })
    }

    /// 記録の全フレーム数。
    #[must_use]
    pub fn frame_count(&self) -> u32 {
        u32::try_from(self.recording.frames.len()).unwrap_or(u32::MAX)
    }
}

/// 記録の読み書きで起きうる失敗。
#[derive(Debug)]
pub enum ReplayError {
    /// 入出力の失敗。
    Io(std::io::Error),
    /// 先頭の識別子が違う。リプレイファイルではない。
    NotAReplay {
        /// 実際に読めた先頭 8 バイト。
        found: [u8; 8],
    },
    /// 形式版が違う。
    UnsupportedVersion {
        /// ファイルにあった版。
        found: u16,
        /// このビルドが読める版。
        expected: u16,
    },
    /// 宣言された個数が受け入れ上限を超えている。
    TooLarge {
        /// 何の個数か。
        what: &'static str,
        /// 宣言値。
        declared: u64,
        /// 上限。
        maximum: u64,
    },
    /// 確保に失敗した。
    OutOfMemory {
        /// 何を確保しようとしたか。
        what: &'static str,
        /// 要素数。
        count: usize,
    },
    /// 機体名が UTF-8 でない。
    InvalidName,
    /// キーフレームのフレーム番号が範囲外、または昇順でない。
    InvalidKeyframe {
        /// 問題のあったフレーム番号。
        frame: u32,
    },
    /// 数値が非有限、定義域外、または計算結果が表現可能な範囲を超える。
    InvalidValue {
        /// 問題のあった項目。
        field: &'static str,
        /// フレーム内の項目なら、その番号。
        frame: Option<u32>,
        /// 満たすべき条件。
        requirement: &'static str,
    },
    /// 再生しようとした条件が記録と違う。
    ConditionsMismatch {
        /// 何が違うか。
        detail: String,
    },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "reading or writing the replay failed: {error}"),
            Self::NotAReplay { found } => write!(
                formatter,
                "this is not a replay file; it starts with {found:02x?} instead of {MAGIC:02x?}"
            ),
            Self::UnsupportedVersion { found, expected } => write!(
                formatter,
                "the replay is format version {found}; this build reads versions 1 through {expected}"
            ),
            Self::TooLarge {
                what,
                declared,
                maximum,
            } => write!(
                formatter,
                "the replay declares {declared} {what}; the safe limit is {maximum}"
            ),
            Self::OutOfMemory { what, count } => write!(
                formatter,
                "could not reserve room for {count} {what}; the file is larger than this machine can hold"
            ),
            Self::InvalidName => write!(formatter, "the aircraft name is not valid UTF-8"),
            Self::InvalidKeyframe { frame } => write!(
                formatter,
                "keyframe {frame} is out of range or out of order; the file is corrupt"
            ),
            Self::InvalidValue {
                field,
                frame,
                requirement,
            } => {
                write!(formatter, "invalid replay {field}")?;
                if let Some(frame) = frame {
                    write!(formatter, " at frame {frame}")?;
                }
                write!(formatter, ": {requirement}")
            }
            Self::ConditionsMismatch { detail } => {
                write!(formatter, "this replay cannot be reproduced here: {detail}")
            }
        }
    }
}

impl std::error::Error for ReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ReplayError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl Recording {
    // Recorder deliberately retains its infallible, permissive API. File
    // boundaries and reproducibility checks validate the resulting recording.
    fn validate(&self) -> Result<(), ReplayError> {
        validate_records(
            &self.conditions.aircraft_name,
            &EnvironmentConditions::from(&self.conditions),
            &self.frames,
            &self.keyframes,
        )
    }

    /// Validate existing recording values/world data and classify aircraft identity.
    ///
    /// All v1/v2 recordings can return only `LegacyPartialMatch` or `Mismatch`.
    /// They never recorded `aero.yaw_rate_p`, so even a matching bundled name and
    /// hash cannot establish that coefficient. This result is evidence, not a
    /// playback policy: callers must explicitly decide whether to permit legacy
    /// partial matches and communicate that uncertainty.
    ///
    /// # Errors
    ///
    /// Invalid recorded values or mismatched bundled world/climate data.
    /// An aircraft identity mismatch is returned as `Ok(Mismatch)`.
    pub fn check_compatibility_with(
        &self,
        config: &AircraftConfig,
    ) -> Result<AircraftCompatibility, ReplayError> {
        self.validate()?;
        if self.conditions.world_terrain
            && self.conditions.terrain_fingerprint != GLOBAL_TERRAIN_FINGERPRINT
        {
            return Err(ReplayError::ConditionsMismatch {
                detail: "the bundled global terrain dataset differs from the recording".to_owned(),
            });
        }
        if self.conditions.climate_date.is_some()
            && self.conditions.climate_fingerprint != GLOBAL_CLIMATE_FINGERPRINT
        {
            return Err(ReplayError::ConditionsMismatch {
                detail: "the bundled climate dataset differs from the recording".to_owned(),
            });
        }
        Ok(self.conditions.aircraft_identity().verify(config))
    }

    /// Historical v1/v2 playback gate retained for source compatibility.
    ///
    /// **`Ok(())` means only legacy partial compatibility**, not complete aircraft
    /// identity or proven reproducibility. The legacy algorithm omits
    /// `aero.yaw_rate_p`. Migrate callers to [`Self::check_compatibility_with`] and
    /// an explicit legacy playback policy before presenting complete verification.
    /// Names and small positional drift cannot recover the missing evidence.
    ///
    /// # Errors
    ///
    /// Invalid recorded values or mismatched aircraft/model/world/climate identity.
    pub fn check_reproducible_with(&self, config: &AircraftConfig) -> Result<(), ReplayError> {
        match self.check_compatibility_with(config)? {
            AircraftCompatibility::CompleteMatch | AircraftCompatibility::LegacyPartialMatch => {
                return Ok(());
            }
            AircraftCompatibility::Mismatch => {}
        }
        let actual = aircraft_fingerprint(config);
        let recorded_name = &self.conditions.aircraft_name;
        Err(ReplayError::ConditionsMismatch {
            detail: format!(
                "aircraft/FDM model mismatch: it was recorded with `{recorded_name}` (fingerprint {:016x}) but `{}` with FDM model revision {FDM_MODEL_REVISION} here has fingerprint {actual:016x}",
                self.conditions.aircraft_fingerprint, config.name
            ),
        })
    }

    /// 書き出す。
    ///
    /// # Errors
    ///
    /// 数値・個数・名前長・キーフレームが不正なとき、および書き込みに失敗したとき。
    /// データの検証は最初の書き込みより前に終える（I/O 自体は非 atomic）。
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        let version = if self.conditions.world_terrain || self.conditions.climate_date.is_some() {
            WORLD_FORMAT_VERSION
        } else {
            LEGACY_FORMAT_VERSION
        };
        self.write_legacy_to(writer, version)
    }

    /// Write exactly format 1, retaining the frozen partial aircraft fingerprint.
    ///
    /// # Errors
    /// Rejects invalid values and enabled world/climate data before writing.
    pub fn write_v1_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        self.write_legacy_to(writer, LEGACY_FORMAT_VERSION)
    }

    /// Write exactly format 2, including its 32-byte world block even if disabled.
    ///
    /// # Errors
    /// Rejects invalid values before writing; I/O can fail after a partial write.
    pub fn write_v2_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        self.write_legacy_to(writer, WORLD_FORMAT_VERSION)
    }

    fn write_legacy_to<W: Write>(&self, writer: &mut W, version: u16) -> Result<(), ReplayError> {
        self.validate()?;
        require_valid(
            version == WORLD_FORMAT_VERSION
                || (!self.conditions.world_terrain && self.conditions.climate_date.is_none()),
            "format 1 world/climate",
            None,
            "format 1 cannot represent enabled world or climate data",
        )?;
        let extended = version == WORLD_FORMAT_VERSION;
        writer.write_all(&MAGIC)?;
        writer.write_all(&version.to_le_bytes())?;

        let name = self.conditions.aircraft_name.as_bytes();
        writer.write_all(&u32::try_from(name.len()).unwrap_or(0).to_le_bytes())?;
        writer.write_all(name)?;

        writer.write_all(&self.conditions.aircraft_fingerprint.to_le_bytes())?;
        write_f64(writer, self.conditions.start.latitude.get())?;
        write_f64(writer, self.conditions.start.longitude.get())?;
        write_f64(writer, self.conditions.start.altitude.get())?;
        write_f64(writer, self.conditions.heading.get())?;
        write_f64(writer, self.conditions.wind.from.get())?;
        write_f64(writer, self.conditions.wind.speed.get())?;
        write_f64(writer, self.conditions.turbulence.intensity.get())?;
        writer.write_all(&self.conditions.turbulence.seed.to_le_bytes())?;
        write_f64(writer, self.conditions.start_epoch)?;
        write_f64(writer, self.conditions.time_rate)?;

        if extended {
            // Fixed 32-byte version-2 extension. Bit 0: global terrain,
            // bit 1: climate. All reserved bits and absent fields are zero.
            let flags = u64::from(self.conditions.world_terrain)
                | (u64::from(self.conditions.climate_date.is_some()) << 1);
            writer.write_all(&flags.to_le_bytes())?;
            writer.write_all(&self.conditions.terrain_fingerprint.to_le_bytes())?;
            write_f64(
                writer,
                self.conditions
                    .climate_date
                    .map_or(0.0, ClimateDate::annual_phase),
            )?;
            writer.write_all(&self.conditions.climate_fingerprint.to_le_bytes())?;
        }

        write_records(writer, &self.frames, &self.keyframes)
    }

    /// 読み込む。
    ///
    /// **壊れた入力で panic しないこと**を前提に書いてある。長さは上限で
    /// 弾き、確保は [`Vec::try_reserve_exact`] で試す。
    ///
    /// # Errors
    ///
    /// 識別子・版・個数・数値の定義域・キーフレームの整合性が崩れているとき、
    /// および読み込みに失敗したとき。
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let mut magic = [0_u8; 8];
        reader.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(ReplayError::NotAReplay { found: magic });
        }
        let version = read_u16(reader)?;
        if !matches!(version, LEGACY_FORMAT_VERSION | WORLD_FORMAT_VERSION) {
            return Err(ReplayError::UnsupportedVersion {
                found: version,
                expected: FORMAT_VERSION,
            });
        }

        let name_len = read_u32(reader)?;
        if name_len > MAX_NAME_BYTES {
            return Err(ReplayError::TooLarge {
                what: "bytes of aircraft name",
                declared: u64::from(name_len),
                maximum: u64::from(MAX_NAME_BYTES),
            });
        }
        let mut name_bytes = vec![0_u8; name_len as usize];
        reader.read_exact(&mut name_bytes)?;
        let aircraft_name = String::from_utf8(name_bytes).map_err(|_| ReplayError::InvalidName)?;

        let aircraft_fingerprint = read_u64(reader)?;
        let latitude = read_f64(reader)?;
        let longitude = read_f64(reader)?;
        let altitude = read_f64(reader)?;
        let heading = read_f64(reader)?;
        let wind_from = read_f64(reader)?;
        let wind_speed = read_f64(reader)?;
        let turbulence_intensity = read_f64(reader)?;
        let turbulence_seed = read_u64(reader)?;
        let start_epoch = read_f64(reader)?;
        let time_rate = read_f64(reader)?;

        let (world_terrain, terrain_fingerprint, climate_date, climate_fingerprint) =
            if version == WORLD_FORMAT_VERSION {
                let flags = read_u64(reader)?;
                let terrain_fingerprint = read_u64(reader)?;
                let phase = read_f64(reader)?;
                let climate_fingerprint = read_u64(reader)?;
                require_valid(
                    flags & !3 == 0,
                    "world/climate flags",
                    None,
                    "only global terrain and climate flag bits are defined",
                )?;
                let climate_date = if flags & 2 != 0 {
                    Some(ClimateDate::from_annual_phase(phase).ok_or(
                        ReplayError::InvalidValue {
                            field: "climate annual phase",
                            frame: None,
                            requirement: "must be finite and in [0, 1)",
                        },
                    )?)
                } else {
                    require_valid(
                        phase.to_bits() == 0,
                        "disabled climate annual phase",
                        None,
                        "must be positive zero when climate is disabled",
                    )?;
                    None
                };
                (
                    flags & 1 != 0,
                    terrain_fingerprint,
                    climate_date,
                    climate_fingerprint,
                )
            } else {
                (false, 0, None, 0)
            };

        let conditions = Conditions {
            aircraft_name,
            aircraft_fingerprint,
            start: Geodetic {
                latitude: Radians(latitude),
                longitude: Radians(longitude),
                altitude: Meters(altitude),
            },
            heading: Radians(heading),
            wind: Wind {
                from: Radians(wind_from),
                speed: MetersPerSecond(wind_speed),
            },
            turbulence: Turbulence {
                intensity: MetersPerSecond(turbulence_intensity),
                seed: turbulence_seed,
            },
            start_epoch,
            time_rate,
            world_terrain,
            terrain_fingerprint,
            climate_date,
            climate_fingerprint,
        };
        validate_conditions(&conditions)?;

        let (frames, keyframes) =
            read_records(reader, conditions.start_epoch, conditions.time_rate)?;

        Ok(Self {
            conditions,
            frames,
            keyframes,
        })
    }
}

/// Validate without changing the stored bits: repairing corruption would make a
/// different flight appear to be the original recording.
fn require_valid(
    valid: bool,
    field: &'static str,
    frame: Option<u32>,
    requirement: &'static str,
) -> Result<(), ReplayError> {
    if valid {
        Ok(())
    } else {
        Err(ReplayError::InvalidValue {
            field,
            frame,
            requirement,
        })
    }
}

fn validate_conditions(conditions: &Conditions) -> Result<(), ReplayError> {
    validate_environment(&EnvironmentConditions::from(conditions))
}

fn validate_environment(conditions: &EnvironmentConditions) -> Result<(), ReplayError> {
    require_valid(
        conditions.world_terrain == (conditions.terrain_fingerprint != 0),
        "terrain fingerprint",
        None,
        "must be nonzero exactly when bundled global terrain is enabled",
    )?;
    require_valid(
        conditions.climate_date.is_some() == (conditions.climate_fingerprint != 0),
        "climate fingerprint",
        None,
        "must be nonzero exactly when climate is enabled",
    )?;
    for (field, value) in [
        ("start latitude", conditions.start.latitude.get()),
        ("start longitude", conditions.start.longitude.get()),
        ("start altitude", conditions.start.altitude.get()),
        ("start heading", conditions.heading.get()),
        ("wind direction", conditions.wind.from.get()),
        ("wind speed", conditions.wind.speed.get()),
        (
            "turbulence intensity",
            conditions.turbulence.intensity.get(),
        ),
        ("start epoch", conditions.start_epoch),
        ("time rate", conditions.time_rate),
    ] {
        require_valid(value.is_finite(), field, None, "must be finite")?;
    }
    require_valid(
        (-std::f64::consts::FRAC_PI_2..=std::f64::consts::FRAC_PI_2)
            .contains(&conditions.start.latitude.get()),
        "start latitude",
        None,
        "must be in [-pi/2, pi/2] radians",
    )?;
    require_valid(
        (-std::f64::consts::PI..=std::f64::consts::PI).contains(&conditions.start.longitude.get()),
        "start longitude",
        None,
        "must be in [-pi, pi] radians",
    )?;
    require_valid(
        conditions.start.to_ecef().0.length_squared().is_finite(),
        "start altitude",
        None,
        "must give a representable squared ECEF magnitude",
    )?;
    for (field, value) in [
        ("wind speed", conditions.wind.speed.get()),
        (
            "turbulence intensity",
            conditions.turbulence.intensity.get(),
        ),
    ] {
        require_valid(
            value >= 0.0 && (value * value).is_finite(),
            field,
            None,
            "must be nonnegative with a representable squared magnitude",
        )?;
    }
    require_valid(
        (0.0..=MAX_VISUAL_EPOCH).contains(&conditions.start_epoch),
        "start epoch",
        None,
        "must be zero (unspecified) or a supported positive Julian date",
    )?;
    require_valid(
        conditions.time_rate >= 0.0,
        "time rate",
        None,
        "must be nonnegative",
    )
}

fn frame_values(frame: &Frame) -> [f64; 7] {
    [
        frame.frame_time.get(),
        frame.controls.aileron(),
        frame.controls.elevator(),
        frame.controls.rudder(),
        frame.controls.throttle(),
        frame.controls.flaps(),
        frame.controls.brakes(),
    ]
}

fn validate_frame_values(values: &[f64; 7], frame: u32) -> Result<(), ReplayError> {
    require_valid(
        values[0].is_finite() && values[0] >= 0.0,
        "frame duration",
        Some(frame),
        "must be finite and nonnegative",
    )?;
    for (index, field) in [
        "aileron", "elevator", "rudder", "throttle", "flaps", "brakes",
    ]
    .into_iter()
    .enumerate()
    {
        let minimum = if index < 3 { -1.0 } else { 0.0 };
        require_valid(
            (minimum..=1.0).contains(&values[index + 1]),
            field,
            Some(frame),
            if index < 3 {
                "must be finite and in [-1, 1]"
            } else {
                "must be finite and in [0, 1]"
            },
        )?;
    }
    Ok(())
}

fn add_duration(elapsed: f64, frame: &Frame, index: u32) -> Result<f64, ReplayError> {
    let total = elapsed + frame.frame_time.get();
    require_valid(
        total.is_finite(),
        "recording duration",
        Some(index),
        "the cumulative duration must be finite",
    )?;
    Ok(total)
}

fn validate_visual_time(
    start_epoch: f64,
    time_rate: f64,
    duration: f64,
) -> Result<(), ReplayError> {
    // Match the app's multiplication-before-division order. A finite epoch and
    // finite rate alone do not protect the derived visual clock from overflow.
    let elapsed = duration * time_rate;
    require_valid(
        elapsed.is_finite(),
        "visual duration",
        None,
        "duration multiplied by time rate must remain finite",
    )?;
    let end = start_epoch + elapsed / 86_400.0;
    require_valid(
        end.is_finite() && end <= MAX_VISUAL_EPOCH,
        "visual end epoch",
        None,
        "must remain within the supported Julian date range",
    )
}

fn validate_keyframe_state(state: &RigidBodyState, frame: u32) -> Result<(), ReplayError> {
    for (field, vector) in [
        ("keyframe position", state.position.0),
        ("keyframe velocity", state.velocity),
        ("keyframe angular velocity", state.angular_velocity),
    ] {
        require_valid(
            vector.is_finite() && vector.length_squared().is_finite(),
            field,
            Some(frame),
            "components and squared magnitude must be finite",
        )?;
    }
    // Preserve the old tolerance for already-unit quaternions, but reject
    // corruption instead of normalizing it or silently substituting identity.
    // In particular, never divide a valid quaternion: its bits are replay data.
    require_valid(
        state.orientation.is_finite() && (state.orientation.length() - 1.0).abs() <= 1e-12,
        "keyframe orientation",
        Some(frame),
        "must be a finite unit quaternion (length tolerance 1e-12)",
    )
}

fn write_f64<W: Write>(writer: &mut W, value: f64) -> std::io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn read_f64<R: Read>(reader: &mut R) -> std::io::Result<f64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(f64::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> std::io::Result<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R) -> std::io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u16<R: Read>(reader: &mut R) -> std::io::Result<u16> {
    let mut bytes = [0_u8; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn write_records<W: Write>(
    writer: &mut W,
    frames: &[Frame],
    keyframes: &[Keyframe],
) -> Result<(), ReplayError> {
    let frame_count = u32::try_from(frames.len()).unwrap_or(u32::MAX);
    let keyframe_count = u32::try_from(keyframes.len()).unwrap_or(u32::MAX);
    writer.write_all(&frame_count.to_le_bytes())?;
    writer.write_all(&keyframe_count.to_le_bytes())?;

    for frame in frames {
        let mut bytes = [0_u8; FRAME_BYTES];
        for (slot, value) in bytes.chunks_exact_mut(8).zip(frame_values(frame)) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        writer.write_all(&bytes)?;
    }

    for keyframe in keyframes {
        writer.write_all(&keyframe.frame.to_le_bytes())?;
        let state = keyframe.state;
        let position = state.position.0;
        for value in [
            position.x,
            position.y,
            position.z,
            state.velocity.x,
            state.velocity.y,
            state.velocity.z,
            state.orientation.x,
            state.orientation.y,
            state.orientation.z,
            state.orientation.w,
            state.angular_velocity.x,
            state.angular_velocity.y,
            state.angular_velocity.z,
        ] {
            write_f64(writer, value)?;
        }
    }
    Ok(())
}

fn read_records<R: Read>(
    reader: &mut R,
    start_epoch: f64,
    time_rate: f64,
) -> Result<(Vec<Frame>, Vec<Keyframe>), ReplayError> {
    let frame_count = read_u32(reader)?;
    if frame_count > MAX_FRAMES {
        return Err(ReplayError::TooLarge {
            what: "frames",
            declared: u64::from(frame_count),
            maximum: u64::from(MAX_FRAMES),
        });
    }
    let keyframe_count = read_u32(reader)?;
    // キーフレームは間隔ごとに 1 つ。上限はそこから決まる。
    let keyframe_limit = frame_count / KEYFRAME_INTERVAL + 1;
    if keyframe_count > keyframe_limit {
        return Err(ReplayError::TooLarge {
            what: "keyframes",
            declared: u64::from(keyframe_count),
            maximum: u64::from(keyframe_limit),
        });
    }

    let mut frames = Vec::new();
    frames
        .try_reserve_exact(frame_count as usize)
        .map_err(|_| ReplayError::OutOfMemory {
            what: "frames",
            count: frame_count as usize,
        })?;
    let mut bytes = [0_u8; FRAME_BYTES];
    let mut duration = 0.0;
    for index in 0..frame_count {
        reader.read_exact(&mut bytes)?;
        let mut values = [0.0_f64; 7];
        for (value, chunk) in values.iter_mut().zip(bytes.chunks_exact(8)) {
            let mut eight = [0_u8; 8];
            eight.copy_from_slice(chunk);
            *value = f64::from_le_bytes(eight);
        }
        // Reject before ControlInputs can silently sanitize corruption.
        validate_frame_values(&values, index)?;
        let frame = Frame {
            frame_time: Seconds(values[0]),
            controls: ControlInputs::new(values[1], values[2], values[3], values[4], values[5])
                .with_brakes(values[6]),
        };
        duration = add_duration(duration, &frame, index)?;
        frames.push(frame);
    }
    validate_visual_time(start_epoch, time_rate, duration)?;

    let mut keyframes = Vec::new();
    keyframes
        .try_reserve_exact(keyframe_count as usize)
        .map_err(|_| ReplayError::OutOfMemory {
            what: "keyframes",
            count: keyframe_count as usize,
        })?;
    let mut keyframe_bytes = [0_u8; KEYFRAME_BYTES];
    let mut previous: Option<u32> = None;
    for _ in 0..keyframe_count {
        reader.read_exact(&mut keyframe_bytes)?;
        let mut four = [0_u8; 4];
        four.copy_from_slice(&keyframe_bytes[..4]);
        let frame = u32::from_le_bytes(four);
        // **範囲外や逆順を通すと `keyframe_at_or_before` の二分探索が
        // 嘘を返す。** 読んだ時点で弾く。
        if frame >= frame_count.max(1) || previous.is_some_and(|last| frame <= last) {
            return Err(ReplayError::InvalidKeyframe { frame });
        }
        previous = Some(frame);
        let mut values = [0.0_f64; 13];
        for (value, chunk) in values.iter_mut().zip(keyframe_bytes[4..].chunks_exact(8)) {
            let mut eight = [0_u8; 8];
            eight.copy_from_slice(chunk);
            *value = f64::from_le_bytes(eight);
        }
        let state = RigidBodyState {
            position: flightsim_core::Ecef::new(values[0], values[1], values[2]),
            velocity: DVec3::new(values[3], values[4], values[5]),
            orientation: DQuat::from_xyzw(values[6], values[7], values[8], values[9]),
            angular_velocity: DVec3::new(values[10], values[11], values[12]),
        };
        validate_keyframe_state(&state, frame)?;
        keyframes.push(Keyframe { frame, state });
    }

    Ok((frames, keyframes))
}

fn validate_records(
    name: &str,
    environment: &EnvironmentConditions,
    frames: &[Frame],
    keyframes: &[Keyframe],
) -> Result<(), ReplayError> {
    for (what, declared, maximum) in [
        (
            "bytes of aircraft name",
            name.len(),
            MAX_NAME_BYTES as usize,
        ),
        ("frames", frames.len(), MAX_FRAMES as usize),
        (
            "keyframes",
            keyframes.len(),
            frames.len() / KEYFRAME_INTERVAL as usize + 1,
        ),
    ] {
        if declared > maximum {
            return Err(ReplayError::TooLarge {
                what,
                declared: u64::try_from(declared).unwrap_or(u64::MAX),
                maximum: u64::try_from(maximum).unwrap_or(u64::MAX),
            });
        }
    }

    validate_environment(environment)?;
    let mut duration = 0.0;
    for (index, frame) in (0_u32..).zip(frames) {
        validate_frame_values(&frame_values(frame), index)?;
        duration = add_duration(duration, frame, index)?;
    }
    validate_visual_time(environment.start_epoch, environment.time_rate, duration)?;
    let mut previous = None;
    for keyframe in keyframes {
        let frame = keyframe.frame;
        if frame as usize >= frames.len().max(1) || previous.is_some_and(|last| frame <= last) {
            return Err(ReplayError::InvalidKeyframe { frame });
        }
        validate_keyframe_state(&keyframe.state, frame)?;
        previous = Some(frame);
    }
    Ok(())
}
