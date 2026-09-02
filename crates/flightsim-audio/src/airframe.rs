//! 機体まわりの音。風切りと失速警報。
//!
//! どちらも 1 標本ずつ作る。エンジンと同じく、状態を持つ構造体に
//! `tick` を生やす形で揃えてある。

use std::f64::consts::TAU;

use crate::dsp::{HighPass, LowPass, Noise, Resonator, SAMPLE_RATE, Smoothed};

/// 風切り音が鳴り始める対気速度 `m/s`。これ以下は無音。
pub const WIND_THRESHOLD: f64 = 8.0;

/// 風切り音が最大になる対気速度 `m/s`。
///
/// この機体の超過禁止速度はモデルに無いので、**巡航より十分速い**ところに
/// 置く。63 m/s ≒ 122 kt。
pub const WIND_FULL_SCALE: f64 = 63.0;

/// 風切り音。
///
/// # なぜ雑音を流すだけでは駄目なのか
///
/// **一定の「シャー」は扇風機にしか聞こえない。** 実際の風切り音は、
/// 速度が上がると音量だけでなく**明るさ（帯域）**が上がる。窓の隙間や
/// 突起で渦ができ、そこに共鳴が乗るためである。
///
/// ここでは、速度でカットオフが動くフィルタと、速度で中心が動く共鳴を
/// 重ねる。音量だけを動かすより、速度が変わったことがはるかに分かりやすい。
#[derive(Debug)]
pub struct WindVoice {
    noise: Noise,
    /// 帯域を決めるローパス。速度で開く。
    body: LowPass,
    /// 低い唸りを削る。**低音が残ると機内の暗騒音と混ざって濁る。**
    rumble_cut: HighPass,
    /// 隙間の共鳴。速度で中心が上がる。
    whistle: Resonator,
    airspeed: Smoothed,
}

impl Default for WindVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl WindVoice {
    /// 作る。
    #[must_use]
    pub fn new() -> Self {
        Self {
            noise: Noise::new(0x5eed_1234_5678_9abc),
            body: LowPass::new(1_200.0),
            rumble_cut: HighPass::new(120.0),
            whistle: Resonator::new(1_600.0, 2.5),
            // 風は急に変わらない。**速度は機体の慣性で動くので、
            // 音も同じくらいゆっくりでよい。**
            airspeed: Smoothed::new(0.0, 0.25),
        }
    }

    /// 開始時の値へ飛ばす。
    pub const fn reset(&mut self, airspeed_ms: f64) {
        self.airspeed.reset(airspeed_ms);
    }

    /// 1 標本作る。
    pub fn tick(&mut self, airspeed_ms: f64) -> f64 {
        let airspeed = self.airspeed.tick(airspeed_ms).max(0.0);
        let position =
            ((airspeed - WIND_THRESHOLD) / (WIND_FULL_SCALE - WIND_THRESHOLD)).clamp(0.0, 1.0);
        if position <= 0.0 {
            return 0.0;
        }

        // 速く飛ぶほど帯域が開く。**明るさが変わることで速度が伝わる。**
        // 上を伸ばしすぎると「シャー」が勝って、機内ではなく
        // ホワイトノイズに聞こえる（実測で巡航時に 4 kHz 超が
        // 全エネルギーの 27% を占めていた）。
        self.body.set_cutoff(position.mul_add(2_600.0, 700.0));
        self.whistle
            .set_frequency(position.mul_add(2_200.0, 900.0), 2.5);

        let raw = self.noise.tick();
        let shaped = self.rumble_cut.tick(self.body.tick(raw));
        let whistle = self.whistle.tick(raw) * position * 0.25;

        // 二乗で効かせる。**線形だと低速から鳴りすぎて、
        // 速度が上がった実感が出ない。**
        (shaped + whistle) * position * position
    }
}

/// 失速警報の高さ `Hz`。
///
/// 実機のリード式ホーンはおおむね数百 Hz から 2 kHz。エンジンの基音
/// （最大でも 90 Hz）とその低次倍音から離しつつ、耳障りすぎない高さとして
/// 800 Hz を採る。
pub const STALL_HORN_HZ: f64 = 800.0;

/// 失速警報。
///
/// 実機のリード式ホーンは、空気で薄板を震わせて鳴らす。**純音ではなく、
/// わずかに揺れた濁りのある音**で、それがエンジン音の中でも埋もれない
/// 理由になっている。
#[derive(Debug)]
pub struct StallHornVoice {
    phase: f64,
    /// リードの揺れ。**完全に一定の音は機械が鳴らしているように聞こえず、
    /// かえって警報として耳に留まらない。**
    wobble_phase: f64,
    resonator: Resonator,
    level: Smoothed,
}

impl Default for StallHornVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl StallHornVoice {
    /// 作る。
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            wobble_phase: 0.0,
            resonator: Resonator::new(STALL_HORN_HZ * 2.0, 8.0),
            // 立ち上がりは速く。**警報が遅れて出ては意味がない。**
            level: Smoothed::new(0.0, 0.02),
        }
    }

    /// 開始時の値へ飛ばす。
    pub const fn reset(&mut self, sounding: bool) {
        self.level.reset(if sounding { 1.0 } else { 0.0 });
    }

    /// 1 標本作る。
    pub fn tick(&mut self, sounding: bool) -> f64 {
        let level = self.level.tick(if sounding { 1.0 } else { 0.0 });
        if level < 1e-4 {
            return 0.0;
        }

        // リードの揺れ。6 Hz で ±1.5%。
        self.wobble_phase = fract(self.wobble_phase + 6.0 / SAMPLE_RATE);
        let wobble = (TAU * self.wobble_phase).sin().mul_add(0.015, 1.0);
        self.phase = fract(self.phase + STALL_HORN_HZ * wobble / SAMPLE_RATE);

        // 矩形に近い波形。リードが叩く音の濁りを出す。
        let angle = TAU * self.phase;
        let reed = angle.sin().mul_add(0.7, (angle * 2.0).sin() * 0.2) + (angle * 3.0).sin() * 0.15;
        // 共鳴を足して「ホーン」らしい鼻にかかった音にする。
        let horned = reed + self.resonator.tick(reed) * 0.45;
        horned * level * 0.5
    }
}

/// `[0, 1)` に折り返す。
fn fract(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let wrapped = value.fract();
    if wrapped < 0.0 {
        wrapped + 1.0
    } else {
        wrapped
    }
}

#[cfg(test)]
// 音の検査では「ちょうど 0（無音）」と「頭打ちで同じ値」が契約そのもの。
// **近似で見ると、無音のはずが極小の音で鳴っていても通ってしまう。**
#[expect(clippy::float_cmp, reason = "無音と頭打ちは厳密な値が契約")]
mod tests {
    use super::*;
    use crate::dsp::tests_support::magnitude_at;

    fn render_wind(airspeed: f64) -> Vec<f64> {
        let mut voice = WindVoice::new();
        voice.reset(airspeed);
        for _ in 0..4_800 {
            voice.tick(airspeed);
        }
        (0..48_000).map(|_| voice.tick(airspeed)).collect()
    }

    fn level(samples: &[f64]) -> f64 {
        (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn a_parked_aircraft_has_no_wind_noise() {
        assert_eq!(level(&render_wind(0.0)), 0.0);
        assert_eq!(level(&render_wind(WIND_THRESHOLD)), 0.0);
    }

    #[test]
    fn the_wind_gets_louder_with_airspeed() {
        let slow = level(&render_wind(20.0));
        let fast = level(&render_wind(50.0));
        assert!(slow > 0.0);
        assert!(fast > slow * 2.0, "{fast} should be well above {slow}");
    }

    #[test]
    fn the_wind_gets_brighter_with_airspeed_not_just_louder() {
        // **音量だけ動かすと、速度が変わった実感が出ない。**
        // 高域と低域の比が速度で変わることを見る。
        let ratio = |airspeed: f64| {
            let samples = render_wind(airspeed);
            let low = magnitude_at(&samples, 300.0).max(1e-12);
            magnitude_at(&samples, 3_000.0) / low
        };
        assert!(
            ratio(50.0) > ratio(20.0) * 1.5,
            "the spectrum should open up with speed: {} vs {}",
            ratio(50.0),
            ratio(20.0)
        );
    }

    #[test]
    fn the_wind_does_not_get_louder_beyond_full_scale() {
        // 頭打ちが無いと、速度超過で割れる。
        let fast = level(&render_wind(WIND_FULL_SCALE));
        let faster = level(&render_wind(500.0));
        assert!((fast - faster).abs() < 1e-9, "{fast} vs {faster}");
    }

    #[test]
    fn the_wind_stays_in_range_and_finite() {
        for airspeed in [0.0, 30.0, 63.0, 300.0, f64::NAN, f64::INFINITY] {
            let mut voice = WindVoice::new();
            let mut peak = 0.0_f64;
            for _ in 0..10_000 {
                let sample = voice.tick(airspeed);
                assert!(sample.is_finite(), "{airspeed} gave {sample}");
                peak = peak.max(sample.abs());
            }
            assert!(peak < 1.5, "{airspeed} peaked at {peak}");
        }
    }

    #[test]
    fn the_horn_is_silent_until_asked() {
        let mut voice = StallHornVoice::new();
        for _ in 0..1_000 {
            assert_eq!(voice.tick(false), 0.0);
        }
    }

    #[test]
    fn the_horn_sounds_at_its_stated_pitch() {
        let mut voice = StallHornVoice::new();
        voice.reset(true);
        for _ in 0..4_800 {
            voice.tick(true);
        }
        let samples: Vec<f64> = (0..48_000).map(|_| voice.tick(true)).collect();
        let horn = magnitude_at(&samples, STALL_HORN_HZ);
        let away = magnitude_at(&samples, STALL_HORN_HZ * 1.4);
        assert!(horn > away * 3.0, "{horn} at the horn vs {away} beside it");
    }

    #[test]
    fn the_horn_comes_up_quickly() {
        // **警報が遅れて出ては意味がない。** 0.1 秒で聞こえていること。
        let mut voice = StallHornVoice::new();
        let mut peak = 0.0_f64;
        for _ in 0..(SAMPLE_RATE as usize / 10) {
            peak = peak.max(voice.tick(true).abs());
        }
        assert!(peak > 0.2, "the horn was still at {peak} after 0.1 s");
    }

    #[test]
    fn the_horn_stops_when_the_warning_clears() {
        let mut voice = StallHornVoice::new();
        for _ in 0..48_000 {
            voice.tick(true);
        }
        let mut peak = 0.0_f64;
        // 止めてから 0.5 秒後の音量を見る。
        for _ in 0..24_000 {
            voice.tick(false);
        }
        for _ in 0..4_800 {
            peak = peak.max(voice.tick(false).abs());
        }
        assert!(peak < 1e-3, "the horn kept sounding at {peak}");
    }

    #[test]
    fn the_horn_is_not_a_pure_tone() {
        // **純音はエンジン音に埋もれる。** 倍音があること。
        let mut voice = StallHornVoice::new();
        voice.reset(true);
        for _ in 0..4_800 {
            voice.tick(true);
        }
        let samples: Vec<f64> = (0..48_000).map(|_| voice.tick(true)).collect();
        let fundamental = magnitude_at(&samples, STALL_HORN_HZ);
        let second = magnitude_at(&samples, STALL_HORN_HZ * 2.0);
        assert!(
            second > fundamental * 0.05,
            "the second harmonic is only {second} against {fundamental}"
        );
    }
}
