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
//! - 機体諸元が変われば別の軌跡になる → 諸元の指紋を記録し、違えば拒否する
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
//! | 後退シーク | [`Player::seek`] が直前のキーフレームを返す。そこから再計算する |
//!
//! **後退シークに近道はない。** 物理は積分なので、戻るには積分し直すしかない。
//! キーフレームはその再計算の開始点を近くに置くためのもの。
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
use flightsim_fdm::{AircraftConfig, ControlInputs, RigidBodyState, Turbulence};
use glam::{DQuat, DVec3};

use crate::simulation::Wind;

/// ファイル先頭の識別子。
pub const MAGIC: [u8; 8] = *b"FSREPLAY";

/// 形式版。**互換性を壊す変更のたびに上げること。**
pub const FORMAT_VERSION: u16 = 1;

/// キーフレームを置く間隔（記録フレーム数）。
///
/// 60 fps なら約 2 秒。後退シークの再計算がこの範囲に収まる。
/// 短くすると容量が増え、長くするとシークが遅くなる。
pub const KEYFRAME_INTERVAL: u32 = 120;

/// 読み込みを受け付けるフレーム数の上限。
///
/// 60 fps で約 4.6 時間。1 フレーム 56 バイトなので、上限まで読んでも約 56 MB。
/// **壊れた長さフィールドで確保しに行かないための線。**
pub const MAX_FRAMES: u32 = 1_000_000;

/// 機体名として受け付けるバイト数の上限。
pub const MAX_NAME_BYTES: u32 = 256;

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
/// `frame_time` は**描画フレーム時間**であって物理の固定 dt ではない。
/// [`crate::Simulation::advance`] が内部で固定 dt に割るので、同じ
/// `frame_time` を渡せば同じ割り方になる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// このフレームで進めた時間。
    pub frame_time: Seconds,
    /// このフレームで与えた操縦入力。
    pub controls: ControlInputs,
}

/// 一定間隔で埋め込む状態の写し。
///
/// 後退シークの開始点であり、**再生がずれていないことの検査点**でもある。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    /// このキーフレームが対応するフレーム番号（このフレームを進める**前**の状態）。
    pub frame: u32,
    /// そのときの剛体状態。
    pub state: RigidBodyState,
}

/// 再生に必要な初期条件。
///
/// **ここが違えば同じ軌跡にならない。** だから全部記録する。
#[derive(Debug, Clone, PartialEq)]
pub struct Conditions {
    /// 機体名。人が読むためのもの。一致判定には使わない。
    pub aircraft_name: String,
    /// 機体諸元の指紋。**これが違えば再生を拒否する。**
    pub aircraft_fingerprint: u64,
    /// 開始位置。
    pub start: Geodetic,
    /// 開始方位。
    pub heading: Radians,
    /// 定常風。
    pub wind: Wind,
    /// 乱流。
    pub turbulence: Turbulence,
    /// 開始時の地方平均太陽時（0 時からの秒）。太陽位置と灯火の再現に要る。
    pub start_time_of_day: Seconds,
    /// 記録時の時間加速率。
    pub time_rate: f64,
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
            start_time_of_day: Seconds(0.0),
            time_rate: 1.0,
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
}

/// 機体諸元の指紋。
///
/// **飛び方を決める数値だけ**を混ぜる。名前は入れない（名前を変えただけで
/// 再生できなくなるのは筋が悪い）。
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
        Seconds(self.frames.iter().map(|frame| frame.frame_time.get()).sum())
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
    cursor: u32,
    paused: bool,
    speed: f64,
    /// 未消化の再生時間。
    budget: Seconds,
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
            cursor: 0,
            paused: false,
            speed: 1.0,
            budget: Seconds(0.0),
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
        self.cursor
    }

    /// 最後まで流し終えたか。
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.cursor as usize >= self.recording.frames.len()
    }

    /// 一時停止しているか。
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// 一時停止・再開。
    ///
    /// 一時停止中は時間を溜めない。**溜めると再開の瞬間に早送りになる。**
    pub const fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.budget = Seconds(0.0);
        }
    }

    /// 再生速度。
    #[must_use]
    pub const fn speed(&self) -> f64 {
        self.speed
    }

    /// 再生速度を変える。[`MIN_SPEED`]〜[`MAX_SPEED`] に丸める。
    ///
    /// NaN は 1 倍に倒す。`f64::clamp` は NaN をそのまま返すので、
    /// 素直に書くと速度が NaN になって再生が止まる。
    pub const fn set_speed(&mut self, speed: f64) {
        self.speed = if speed.is_nan() {
            1.0
        } else {
            speed.clamp(MIN_SPEED, MAX_SPEED)
        };
    }

    /// 描画フレームごとに 1 回呼び、進めてよい時間を足す。
    pub fn accumulate(&mut self, real_frame_time: Seconds) {
        if self.paused || !real_frame_time.get().is_finite() || real_frame_time.get() <= 0.0 {
            return;
        }
        self.budget = Seconds(self.budget.get() + real_frame_time.get() * self.speed);
    }

    /// 溜めた時間の範囲で次のフレームを 1 つ配る。無ければ `None`。
    ///
    /// 予算が尽きるまで繰り返し呼ぶ。**時間を足すのは
    /// [`Self::accumulate`] だけ**なので、何度呼んでも二重に進まない。
    pub fn next_due(&mut self) -> Option<Frame> {
        if self.paused {
            return None;
        }
        let frame = *self.recording.frames.get(self.cursor as usize)?;
        if self.budget.get() < frame.frame_time.get() {
            return None;
        }
        self.budget = Seconds(self.budget.get() - frame.frame_time.get());
        self.cursor = self.cursor.saturating_add(1);
        Some(frame)
    }

    /// 一時停止に関係なく次のフレームを 1 つ取り出す。シークの空回しに使う。
    pub fn step_once(&mut self) -> Option<Frame> {
        let frame = *self.recording.frames.get(self.cursor as usize)?;
        self.cursor = self.cursor.saturating_add(1);
        Some(frame)
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
        self.cursor = keyframe.frame;
        self.budget = Seconds(0.0);
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
                "the replay is format version {found}; this build reads version {expected}"
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
    /// 再生しようとしている機体で、この記録が再現できるか調べる。
    ///
    /// **名前ではなく指紋で見る。** 同じ名前で係数を書き換えた機体で再生すると
    /// 別の軌跡になり、リプレイが嘘をつく。
    ///
    /// # Errors
    ///
    /// 諸元の指紋が一致しないとき [`ReplayError::ConditionsMismatch`]。
    pub fn check_reproducible_with(&self, config: &AircraftConfig) -> Result<(), ReplayError> {
        let actual = aircraft_fingerprint(config);
        if actual == self.conditions.aircraft_fingerprint {
            return Ok(());
        }
        let recorded_name = &self.conditions.aircraft_name;
        Err(ReplayError::ConditionsMismatch {
            detail: format!(
                "it was recorded with `{recorded_name}` (fingerprint {:016x}) but `{}` here has fingerprint {actual:016x}",
                self.conditions.aircraft_fingerprint, config.name
            ),
        })
    }

    /// 書き出す。
    ///
    /// # Errors
    ///
    /// 書き込みに失敗したとき [`ReplayError::Io`]。
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        writer.write_all(&MAGIC)?;
        writer.write_all(&FORMAT_VERSION.to_le_bytes())?;

        let name = self.conditions.aircraft_name.as_bytes();
        let name = &name[..name.len().min(MAX_NAME_BYTES as usize)];
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
        write_f64(writer, self.conditions.start_time_of_day.get())?;
        write_f64(writer, self.conditions.time_rate)?;

        let frame_count = u32::try_from(self.frames.len()).unwrap_or(u32::MAX);
        let keyframe_count = u32::try_from(self.keyframes.len()).unwrap_or(u32::MAX);
        writer.write_all(&frame_count.to_le_bytes())?;
        writer.write_all(&keyframe_count.to_le_bytes())?;

        for frame in &self.frames {
            let mut bytes = [0_u8; FRAME_BYTES];
            let controls = frame.controls;
            for (slot, value) in bytes.chunks_exact_mut(8).zip([
                frame.frame_time.get(),
                controls.aileron(),
                controls.elevator(),
                controls.rudder(),
                controls.throttle(),
                controls.flaps(),
                controls.brakes(),
            ]) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            writer.write_all(&bytes)?;
        }

        for keyframe in &self.keyframes {
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

    /// 読み込む。
    ///
    /// **壊れた入力で panic しないこと**を前提に書いてある。長さは上限で
    /// 弾き、確保は [`Vec::try_reserve_exact`] で試す。
    ///
    /// # Errors
    ///
    /// 識別子・版・個数・キーフレームの整合性のいずれかが崩れているとき、
    /// および読み込みに失敗したとき。
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let mut magic = [0_u8; 8];
        reader.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(ReplayError::NotAReplay { found: magic });
        }
        let version = read_u16(reader)?;
        if version != FORMAT_VERSION {
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
        let start_time_of_day = read_f64(reader)?;
        let time_rate = read_f64(reader)?;

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
            start_time_of_day: Seconds(start_time_of_day),
            time_rate,
        };

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
        for _ in 0..frame_count {
            reader.read_exact(&mut bytes)?;
            let mut values = [0.0_f64; 7];
            for (value, chunk) in values.iter_mut().zip(bytes.chunks_exact(8)) {
                let mut eight = [0_u8; 8];
                eight.copy_from_slice(chunk);
                *value = f64::from_le_bytes(eight);
            }
            frames.push(Frame {
                // 入力の範囲外・NaN は `ControlInputs` が潰す。
                frame_time: Seconds(values[0]),
                controls: ControlInputs::new(values[1], values[2], values[3], values[4], values[5])
                    .with_brakes(values[6]),
            });
        }

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
            keyframes.push(Keyframe {
                frame,
                state: RigidBodyState {
                    position: flightsim_core::Ecef::new(values[0], values[1], values[2]),
                    velocity: DVec3::new(values[3], values[4], values[5]),
                    // 正規化する。ビットが 1 つ壊れただけで回転が発散する。
                    orientation: normalized_or_identity(DQuat::from_xyzw(
                        values[6], values[7], values[8], values[9],
                    )),
                    angular_velocity: DVec3::new(values[10], values[11], values[12]),
                },
            });
        }

        Ok(Self {
            conditions,
            frames,
            keyframes,
        })
    }
}

/// 正規化した回転。長さが 0 や非有限なら無回転に倒す。
fn normalized_or_identity(quaternion: DQuat) -> DQuat {
    let length = quaternion.length();
    if length.is_finite() && length > 1e-9 {
        DQuat::from_xyzw(
            quaternion.x / length,
            quaternion.y / length,
            quaternion.z / length,
            quaternion.w / length,
        )
    } else {
        DQuat::IDENTITY
    }
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
