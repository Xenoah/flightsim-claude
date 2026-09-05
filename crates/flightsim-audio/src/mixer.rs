//! 3 つの音源をまとめて 1 本の流れにする。
//!
//! **Bevy に依存しない。** ここまでが純 Rust で、Bevy へ繋ぐのは
//! [`crate::source`] の仕事。おかげで音そのものは GUI 無しで検査できる。

use crate::airframe::{StallHornVoice, WindVoice};
use crate::dsp::{Smoothed, soft_clip};
use crate::engine::{EngineSpec, EngineVoice, estimate_rpm};
use crate::turbine::{TurbineSpec, TurbineVoice, estimate_n1};

/// 音の入力。機体の状態をそのまま渡す。
///
/// **音量や周波数ではなく物理量を渡す。** ここで「どういう音にするか」を
/// 決めさせると、判断が呼び出し側に散る。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlightSound {
    /// 出力 `[0, 1]`。
    pub throttle: f64,
    /// 対気速度 `m/s`。
    pub airspeed: f64,
    /// 失速警報を鳴らすか。
    pub stall_warning: bool,
    /// 黙らせるか。一時停止・墜落で立てる。
    pub muted: bool,
}

/// 全体の音量。
///
/// 控えめに始める。**起動していきなり大音量は事故**で、下げる手段を
/// 探す前にスピーカーを切られる。
pub const DEFAULT_MASTER: f64 = 0.5;

/// どの動力を鳴らすか。
///
/// **音だけの選択で、飛び方は変わらない。** FDM は 160 hp のピストン単発を
/// 解いており、ここでタービンを選んでも推力も速度も変わらない。
/// 音と物理が食い違うことを承知のうえで選ぶこと。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineKind {
    /// ピストン単発 + プロペラ。**FDM が解いている機体はこちら。**
    Piston(EngineSpec),
    /// 低バイパス比ターボファン（戦闘機）。
    Turbine(TurbineSpec),
}

impl Default for EngineKind {
    fn default() -> Self {
        Self::Turbine(TurbineSpec::default())
    }
}

impl EngineKind {
    /// 名前から読む。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_lowercase().as_str() {
            "piston" | "prop" | "propeller" => Some(Self::Piston(EngineSpec::default())),
            "turbine" | "jet" | "turbofan" | "fighter" => {
                Some(Self::Turbine(TurbineSpec::default()))
            }
            _ => None,
        }
    }

    /// 表示名。**ASCII のみ**（ログと画面に出る）。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Piston(_) => "piston",
            Self::Turbine(_) => "turbine",
        }
    }
}

/// 動力の音を作る側。機種ごとに中身が違う。
#[derive(Debug)]
enum PowerplantVoice {
    Piston(Box<EngineVoice>),
    Turbine(Box<TurbineVoice>),
}

impl PowerplantVoice {
    fn new(kind: EngineKind) -> Self {
        match kind {
            EngineKind::Piston(spec) => Self::Piston(Box::new(EngineVoice::new(spec))),
            EngineKind::Turbine(spec) => Self::Turbine(Box::new(TurbineVoice::new(spec))),
        }
    }

    fn reset(&mut self, throttle: f64, airspeed: f64) {
        match self {
            Self::Piston(voice) => {
                let rpm = estimate_rpm(voice.spec(), throttle, airspeed);
                voice.reset(rpm, throttle);
            }
            Self::Turbine(voice) => {
                let n1 = estimate_n1(voice.spec(), throttle);
                voice.reset(n1, throttle);
            }
        }
    }

    fn tick(&mut self, throttle: f64, airspeed: f64) -> f64 {
        match self {
            Self::Piston(voice) => {
                let rpm = estimate_rpm(voice.spec(), throttle, airspeed);
                voice.tick(rpm, throttle)
            }
            Self::Turbine(voice) => {
                let n1 = estimate_n1(voice.spec(), throttle);
                voice.tick(n1, throttle)
            }
        }
    }

    /// 今の回転数。ピストンは rpm、タービンは N1。
    const fn speed(&self) -> f64 {
        match self {
            Self::Piston(voice) => voice.rpm(),
            Self::Turbine(voice) => voice.n1(),
        }
    }

    /// 混ぜるときの比。**タービンは元から大きい音なので控える。**
    const fn mix_gain(&self) -> f64 {
        match self {
            Self::Piston(_) => 1.05,
            Self::Turbine(_) => 0.85,
        }
    }
}

/// 3 つの音源を混ぜる。
#[derive(Debug)]
pub struct Mixer {
    engine: PowerplantVoice,
    wind: WindVoice,
    horn: StallHornVoice,
    kind: EngineKind,
    /// 全体の音量。**急に変えると段差が出る**のでなめらかに追わせる。
    master: Smoothed,
    /// 消音。同じく段差を出さないため滑らかに。
    gate: Smoothed,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new(EngineKind::default(), DEFAULT_MASTER)
    }
}

impl Mixer {
    /// 機種と音量を決めて作る。
    #[must_use]
    pub fn new(kind: EngineKind, master: f64) -> Self {
        Self {
            engine: PowerplantVoice::new(kind),
            wind: WindVoice::new(),
            horn: StallHornVoice::new(),
            kind,
            master: Smoothed::new(clamp_unit(master), 0.05),
            // 消音は素早く。ただし段差は作らない。
            gate: Smoothed::new(1.0, 0.02),
        }
    }

    /// どの動力を鳴らしているか。
    #[must_use]
    pub const fn kind(&self) -> EngineKind {
        self.kind
    }

    /// 今なぞっている回転数。ピストンは rpm、タービンは N1。
    #[must_use]
    pub const fn speed(&self) -> f64 {
        self.engine.speed()
    }

    /// 状態を即座に反映する。**やり直しの瞬間だけ**（渡り音を出さないため）。
    pub fn reset(&mut self, input: FlightSound) {
        self.engine.reset(input.throttle, input.airspeed);
        self.wind.reset(input.airspeed);
        self.horn.reset(input.stall_warning);
        self.gate.reset(if input.muted { 0.0 } else { 1.0 });
    }

    /// 1 標本作る。
    pub fn tick(&mut self, input: FlightSound, master: f64) -> f64 {
        let master = self.master.tick(clamp_unit(master));
        let gate = self.gate.tick(if input.muted { 0.0 } else { 1.0 });

        // **黙っている間も音源は回し続ける。** 止めて作り直すと、
        // 再開のたびに位相が飛んで「ブツッ」と鳴る。
        let engine = self.engine.tick(input.throttle, input.airspeed);
        let wind = self.wind.tick(input.airspeed);
        // 警報はほかの音に埋もれてはいけない。**大きめに出す。**
        let horn = self.horn.tick(input.stall_warning);

        // 混ぜる比。**エンジンが主で、風は下敷き、警報は最優先。**
        // 実測で巡航時の峰が 0.21 しかなく小さかったので、全体を上げてある。
        let mixed = engine.mul_add(self.engine.mix_gain(), wind * 0.34) + horn * 0.85;
        // 足し合わせが 1 を超えても頭を切らない。
        soft_clip(mixed) * master * gate
    }
}

fn clamp_unit(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
// 音の検査では「ちょうど 0（無音）」と「頭打ちで同じ値」が契約そのもの。
// **近似で見ると、無音のはずが極小の音で鳴っていても通ってしまう。**
#[expect(clippy::float_cmp, reason = "無音と頭打ちは厳密な値が契約")]
mod tests {
    use super::*;
    use crate::dsp::{SAMPLE_RATE, tests_support::magnitude_at};

    fn flying(throttle: f64, airspeed: f64) -> FlightSound {
        FlightSound {
            throttle,
            airspeed,
            stall_warning: false,
            muted: false,
        }
    }

    fn render(input: FlightSound, seconds: f64) -> Vec<f64> {
        let mut mixer = Mixer::default();
        mixer.reset(input);
        for _ in 0..(SAMPLE_RATE as usize / 4) {
            mixer.tick(input, DEFAULT_MASTER);
        }
        (0..(SAMPLE_RATE * seconds) as usize)
            .map(|_| mixer.tick(input, DEFAULT_MASTER))
            .collect()
    }

    fn level(samples: &[f64]) -> f64 {
        (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn the_mix_never_leaves_the_speaker_range() {
        // **1 を超えた標本は出力側で切られて歪む。**
        for throttle in [0.0, 0.5, 1.0] {
            for airspeed in [0.0, 40.0, 90.0] {
                let mut input = flying(throttle, airspeed);
                input.stall_warning = true;
                let samples = render(input, 0.3);
                let peak = samples.iter().fold(0.0_f64, |peak, s| peak.max(s.abs()));
                assert!(
                    peak <= 1.0,
                    "peak {peak} at throttle {throttle}, airspeed {airspeed}"
                );
            }
        }
    }

    #[test]
    fn everything_at_once_still_leaves_the_horn_audible() {
        // **警報が聞こえなければ意味がない。** 全開・高速でも 800 Hz が立つこと。
        let mut loud = flying(1.0, 70.0);
        loud.stall_warning = true;
        let with_horn = render(loud, 1.0);

        let mut without = loud;
        without.stall_warning = false;
        let no_horn = render(without, 1.0);

        let sounding = magnitude_at(&with_horn, 800.0);
        let quiet = magnitude_at(&no_horn, 800.0);
        assert!(
            sounding > quiet * 3.0,
            "the horn should cut through: {sounding} against {quiet}"
        );
    }

    #[test]
    fn muting_silences_the_mix() {
        let mut muted = flying(1.0, 60.0);
        muted.stall_warning = true;
        muted.muted = true;
        let samples = render(muted, 0.3);
        assert!(
            level(&samples) < 1e-4,
            "still audible at {}",
            level(&samples)
        );
    }

    #[test]
    fn muting_does_not_click() {
        // **音源を止めて作り直すと、再開のたびに「ブツッ」と鳴る。**
        // 消音は音量を落とすだけで、位相は回り続けること。
        //
        // 絶対値で閾値を置いてはいけない。音そのものが 1 標本あたり
        // 0.05 程度は動く（4 kHz の成分があれば当然そうなる）ので、
        // **鳴らしているときの段差と比べる**。消音が段差を増やして
        // いなければ、そこにクリックは無い。
        let flying_input = flying(0.8, 50.0);

        let largest_step = |samples: &[f64]| {
            samples
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).abs())
                .fold(0.0_f64, f64::max)
        };

        let mut mixer = Mixer::default();
        mixer.reset(flying_input);
        for _ in 0..24_000 {
            mixer.tick(flying_input, DEFAULT_MASTER);
        }
        let playing: Vec<f64> = (0..24_000)
            .map(|_| mixer.tick(flying_input, DEFAULT_MASTER))
            .collect();

        let mut muted = flying_input;
        muted.muted = true;
        let muting: Vec<f64> = (0..24_000)
            .map(|_| mixer.tick(muted, DEFAULT_MASTER))
            .collect();

        assert!(
            largest_step(&muting) <= largest_step(&playing),
            "muting added a jump: {} against {} while playing",
            largest_step(&muting),
            largest_step(&playing)
        );
        // 実際に黙ったことも確かめる。段差が無いだけでは検査にならない。
        assert!(level(&muting[12_000..]) < 1e-4);
    }

    #[test]
    fn the_master_volume_scales_the_whole_mix() {
        let input = flying(0.8, 50.0);
        let render_at = |master: f64| {
            let mut mixer = Mixer::new(EngineKind::default(), master);
            mixer.reset(input);
            for _ in 0..12_000 {
                mixer.tick(input, master);
            }
            let samples: Vec<f64> = (0..24_000).map(|_| mixer.tick(input, master)).collect();
            level(&samples)
        };
        assert!(render_at(0.25) < render_at(0.5));
        assert_eq!(render_at(0.0), 0.0);
    }

    #[test]
    fn broken_inputs_do_not_reach_the_speakers() {
        // **NaN が混ざると、鳴り止まない雑音になることがある。**
        let mut mixer = Mixer::default();
        let broken = FlightSound {
            throttle: f64::NAN,
            airspeed: f64::INFINITY,
            stall_warning: true,
            muted: false,
        };
        for _ in 0..10_000 {
            let sample = mixer.tick(broken, f64::NAN);
            assert!(sample.is_finite(), "got {sample}");
            assert!((-1.0..=1.0).contains(&sample));
        }
        // 壊れた値のあとでも鳴り直せること。
        let sane = flying(0.8, 50.0);
        let mut peak = 0.0_f64;
        for _ in 0..48_000 {
            peak = peak.max(mixer.tick(sane, DEFAULT_MASTER).abs());
        }
        assert!(peak > 0.01, "the mixer went silent for good, peak {peak}");
    }

    #[test]
    fn the_output_is_the_same_every_time() {
        // **同じ設定で違う音が鳴ると、不具合と区別が付かない。**
        let input = flying(0.7, 40.0);
        assert_eq!(render(input, 0.2), render(input, 0.2));
    }

    #[test]
    fn opening_the_throttle_makes_it_louder() {
        let quiet = level(&render(flying(0.0, 0.0), 0.5));
        let loud = level(&render(flying(1.0, 0.0), 0.5));
        assert!(loud > quiet * 1.5, "{loud} against {quiet}");
        assert!(quiet > 0.0, "idle must still be audible");
    }

    #[test]
    fn the_engine_kind_is_selectable_by_name() {
        assert!(matches!(
            EngineKind::parse("turbine"),
            Some(EngineKind::Turbine(_))
        ));
        assert!(matches!(
            EngineKind::parse("fighter"),
            Some(EngineKind::Turbine(_))
        ));
        assert!(matches!(
            EngineKind::parse("piston"),
            Some(EngineKind::Piston(_))
        ));
        assert!(matches!(
            EngineKind::parse("  PROP "),
            Some(EngineKind::Piston(_))
        ));
        assert!(EngineKind::parse("rocket").is_none());
    }

    #[test]
    fn the_engine_names_are_ascii() {
        // ログと画面に出る。既定フォントに字形の無い記号を混ぜない。
        for kind in [
            EngineKind::Piston(EngineSpec::default()),
            EngineKind::Turbine(crate::turbine::TurbineSpec::default()),
        ] {
            assert!(kind.name().is_ascii(), "{}", kind.name());
        }
    }

    #[test]
    fn the_turbine_sings_far_higher_than_the_piston() {
        // **これが「キーン」かどうかの分かれ目。**
        // ピストンの点火は最大 90 Hz、タービンのファン翼通過音は数 kHz。
        let render_kind = |kind: EngineKind| {
            let mut mixer = Mixer::new(kind, DEFAULT_MASTER);
            let input = flying(1.0, 60.0);
            mixer.reset(input);
            for _ in 0..(SAMPLE_RATE as usize * 6) {
                mixer.tick(input, DEFAULT_MASTER);
            }
            (0..SAMPLE_RATE as usize)
                .map(|_| mixer.tick(input, DEFAULT_MASTER))
                .collect::<Vec<_>>()
        };

        let piston = render_kind(EngineKind::Piston(EngineSpec::default()));
        let turbine = render_kind(EngineKind::Turbine(crate::turbine::TurbineSpec::default()));

        // 高域の強さで比べる。
        //
        // **狙い撃ちの周波数で測ってはいけない。** ファンの翼通過音は
        // 7077 Hz と鋭く、7000 Hz を見ただけでは完全に外す（一度そうやって
        // 「タービンの方が暗い」という結果を出した）。帯域で測る。
        let high = |samples: &[f64]| {
            let mut filter = crate::dsp::HighPass::new(3_000.0);
            let energy: f64 = samples.iter().map(|s| filter.tick(*s).powi(2)).sum();
            (energy / samples.len() as f64).sqrt()
        };
        // 実測 2.5 倍（3 kHz 以上の実効値、全開・60 m/s）。風切り音は
        // どちらにも同じだけ乗るので、その分だけ差が薄まる。
        // 2 倍を割るなら、ファンの翼通過音が排気の広帯域に埋もれている。
        assert!(
            high(&turbine) > high(&piston) * 2.0,
            "the turbine should be far brighter: {} against {}",
            high(&turbine),
            high(&piston)
        );
    }

    #[test]
    fn both_engine_kinds_stay_within_the_speaker_range() {
        for kind in [
            EngineKind::Piston(EngineSpec::default()),
            EngineKind::Turbine(crate::turbine::TurbineSpec::default()),
        ] {
            for throttle in [0.0, 0.5, 0.95, 1.0] {
                let mut mixer = Mixer::new(kind, DEFAULT_MASTER);
                let mut input = flying(throttle, 70.0);
                input.stall_warning = true;
                mixer.reset(input);
                let mut peak = 0.0_f64;
                for _ in 0..(SAMPLE_RATE as usize * 2) {
                    peak = peak.max(mixer.tick(input, DEFAULT_MASTER).abs());
                }
                assert!(
                    peak <= 1.0,
                    "{kind:?} peaked at {peak} on throttle {throttle}"
                );
            }
        }
    }

    #[test]
    fn there_is_no_dc_offset_in_the_mix() {
        // **直流はスピーカーを押しっぱなしにする。** 聞こえないまま歪みだけが増える。
        let samples = render(flying(0.8, 50.0), 1.0);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean.abs() < 0.01, "dc offset {mean}");
    }
}
