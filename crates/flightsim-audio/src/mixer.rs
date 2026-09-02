//! 3 つの音源をまとめて 1 本の流れにする。
//!
//! **Bevy に依存しない。** ここまでが純 Rust で、Bevy へ繋ぐのは
//! [`crate::source`] の仕事。おかげで音そのものは GUI 無しで検査できる。

use crate::airframe::{StallHornVoice, WindVoice};
use crate::dsp::{Smoothed, soft_clip};
use crate::engine::{EngineSpec, EngineVoice, estimate_rpm};

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

/// 3 つの音源を混ぜる。
#[derive(Debug)]
pub struct Mixer {
    engine: EngineVoice,
    wind: WindVoice,
    horn: StallHornVoice,
    spec: EngineSpec,
    /// 全体の音量。**急に変えると段差が出る**のでなめらかに追わせる。
    master: Smoothed,
    /// 消音。同じく段差を出さないため滑らかに。
    gate: Smoothed,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new(EngineSpec::default(), DEFAULT_MASTER)
    }
}

impl Mixer {
    /// 諸元と音量を決めて作る。
    #[must_use]
    pub fn new(spec: EngineSpec, master: f64) -> Self {
        Self {
            engine: EngineVoice::new(spec),
            wind: WindVoice::new(),
            horn: StallHornVoice::new(),
            spec,
            master: Smoothed::new(clamp_unit(master), 0.05),
            // 消音は素早く。ただし段差は作らない。
            gate: Smoothed::new(1.0, 0.02),
        }
    }

    /// エンジンの諸元。
    #[must_use]
    pub const fn spec(&self) -> EngineSpec {
        self.spec
    }

    /// 今なぞっている回転数。表示と検査に使う。
    #[must_use]
    pub const fn rpm(&self) -> f64 {
        self.engine.rpm()
    }

    /// 状態を即座に反映する。**やり直しの瞬間だけ**（渡り音を出さないため）。
    pub fn reset(&mut self, input: FlightSound) {
        let rpm = estimate_rpm(self.spec, input.throttle, input.airspeed);
        self.engine.reset(rpm, input.throttle);
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
        let rpm = estimate_rpm(self.spec, input.throttle, input.airspeed);
        let engine = self.engine.tick(rpm, input.throttle);
        let wind = self.wind.tick(input.airspeed);
        // 警報はほかの音に埋もれてはいけない。**大きめに出す。**
        let horn = self.horn.tick(input.stall_warning);

        // 混ぜる比。**エンジンが主で、風は下敷き、警報は最優先。**
        // 実測で巡航時の峰が 0.21 しかなく小さかったので、全体を上げてある。
        let mixed = engine.mul_add(1.05, wind * 0.34) + horn * 0.85;
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
            let mut mixer = Mixer::new(EngineSpec::default(), master);
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
    fn there_is_no_dc_offset_in_the_mix() {
        // **直流はスピーカーを押しっぱなしにする。** 聞こえないまま歪みだけが増える。
        let samples = render(flying(0.8, 50.0), 1.0);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean.abs() < 0.01, "dc offset {mean}");
    }
}
