//! Version-preserving player with the same clock/cursor semantics as legacy Player.
use super::{Frame, MAX_SPEED, MIN_SPEED, ReplayFile, SeekPlan};
use flightsim_core::{Meters, Seconds};

/// Private timing state shared by both public players. It owns no recording and
/// therefore cannot replace or upgrade the source file's aircraft evidence.
#[derive(Debug, Clone)]
pub(super) struct PlaybackCursor {
    pub(super) cursor: u32,
    pub(super) paused: bool,
    pub(super) speed: f64,
    budget: Seconds,
}
impl PlaybackCursor {
    pub(super) const fn new() -> Self {
        Self {
            cursor: 0,
            paused: false,
            speed: 1.0,
            budget: Seconds(0.0),
        }
    }
    pub(super) const fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.budget = Seconds(0.0);
        }
    }
    pub(super) const fn set_speed(&mut self, speed: f64) {
        self.speed = if speed.is_nan() {
            1.0
        } else {
            speed.clamp(MIN_SPEED, MAX_SPEED)
        };
    }
    pub(super) fn accumulate(&mut self, real_frame_time: Seconds) {
        if self.paused || !real_frame_time.get().is_finite() || real_frame_time.get() <= 0.0 {
            return;
        }
        let budget = self.budget.get() + real_frame_time.get() * self.speed;
        if budget.is_finite() {
            self.budget = Seconds(budget);
        }
    }
    pub(super) fn next_due(&mut self, frames: &[Frame]) -> Option<Frame> {
        if self.paused {
            return None;
        }
        let frame = *frames.get(self.cursor as usize)?;
        if self.budget.get() < frame.frame_time.get() {
            return None;
        }
        self.budget = Seconds(self.budget.get() - frame.frame_time.get());
        self.cursor = self.cursor.saturating_add(1);
        Some(frame)
    }
    pub(super) fn step_once(&mut self, frames: &[Frame]) -> Option<Frame> {
        let frame = *frames.get(self.cursor as usize)?;
        self.cursor = self.cursor.saturating_add(1);
        Some(frame)
    }
    pub(super) const fn seek(&mut self, frame: u32) {
        self.cursor = frame;
        self.budget = Seconds(0.0);
    }
}

/// Playback cursor over an explicit V1/V2/V3 source. Import/export through
/// `recording()` preserves its version and original identity evidence. This
/// player does not grant app compatibility or weather-presentation permission.
#[derive(Debug, Clone)]
pub struct ReplayFilePlayer {
    recording: ReplayFile,
    playback: PlaybackCursor,
}

impl ReplayFilePlayer {
    /// これ以上離れたら「別の飛行になった」と見なす目安 `m`。
    ///
    /// 積分の丸めだけなら数分飛んでも 1 m には届かない。これを超えるのは
    /// **地形か機体か物理が違う**ということ。値は判断の目安であって、
    /// この型は超えても勝手に止めない。
    pub const DIVERGENCE_LIMIT: Meters = Meters(50.0);

    /// 記録を読み込んで先頭に置く。
    #[must_use]
    pub const fn new(recording: ReplayFile) -> Self {
        Self {
            recording,
            playback: PlaybackCursor::new(),
        }
    }

    /// 再生元の記録。
    #[must_use]
    pub const fn recording(&self) -> &ReplayFile {
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
        self.playback.cursor as usize >= self.recording.frames().len()
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
        u32::try_from(self.recording.frames().len()).unwrap_or(u32::MAX)
    }
}
