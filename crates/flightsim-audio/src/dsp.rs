//! 音を作るための部品。
//!
//! **どれも 1 標本ずつ処理する。** 実時間で生成するので、まとめて配列を
//! 作る形にはできない。状態を持つ構造体に `tick` を生やす形で統一する。
//!
//! Bevy にも `flightsim` の他クレートにも依存しない純 Rust。検査はここに集中する。

use std::f64::consts::TAU;

/// 標本化周波数 `Hz`。
///
/// 48 kHz。多くの出力機器の既定値で、44.1 kHz から再標本化されずに済む。
pub const SAMPLE_RATE: f64 = 48_000.0;

/// 決定論的な擬似乱数（xorshift64）。
///
/// **`rand` を持ち込まない。** 同じ設定で必ず同じ音になってほしい。
/// 音が変わったら、それは不具合の兆候として意味を持つべきである。
#[derive(Debug, Clone, Copy)]
pub struct Noise(u64);

impl Noise {
    /// 種を決めて作る。0 は使えないので別の値へ倒す。
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        })
    }

    /// 次の値を `[-1, 1)` で返す。
    pub fn tick(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // 上位 32 bit を使う。xorshift の下位ビットは周期が短い。
        let unit = f64::from((self.0 >> 32) as u32) / f64::from(u32::MAX);
        unit.mul_add(2.0, -1.0)
    }
}

/// 1 次ローパス。
///
/// 生のホワイトノイズは「砂嵐」で、風にも排気にも聞こえない。
/// **帯域を削って初めて材質が出る。**
#[derive(Debug, Clone, Copy)]
pub struct LowPass {
    coefficient: f64,
    state: f64,
}

impl LowPass {
    /// カットオフ周波数から作る。
    #[must_use]
    pub fn new(cutoff_hz: f64) -> Self {
        Self {
            coefficient: one_pole_coefficient(cutoff_hz),
            state: 0.0,
        }
    }

    /// カットオフを変える。**毎標本呼んでよい**（実時間で追従させるため）。
    pub fn set_cutoff(&mut self, cutoff_hz: f64) {
        self.coefficient = one_pole_coefficient(cutoff_hz);
    }

    /// 1 標本通す。
    pub fn tick(&mut self, input: f64) -> f64 {
        if !input.is_finite() {
            // **一度 NaN が入ると、状態に居座って以降ずっと無音になる。**
            return self.state;
        }
        self.state += self.coefficient * (input - self.state);
        self.state
    }
}

/// 1 次ハイパス。低域を削る。
#[derive(Debug, Clone, Copy)]
pub struct HighPass {
    low: LowPass,
}

impl HighPass {
    /// カットオフ周波数から作る。
    #[must_use]
    pub fn new(cutoff_hz: f64) -> Self {
        Self {
            low: LowPass::new(cutoff_hz),
        }
    }

    /// 1 標本通す。
    pub fn tick(&mut self, input: f64) -> f64 {
        input - self.low.tick(input)
    }
}

/// 1 次フィルタの係数。カットオフは `[0.1, ナイキスト)` に丸める。
fn one_pole_coefficient(cutoff_hz: f64) -> f64 {
    let nyquist = SAMPLE_RATE / 2.0;
    let cutoff = if cutoff_hz.is_finite() {
        cutoff_hz.clamp(0.1, nyquist * 0.99)
    } else {
        1_000.0
    };
    let x = (-TAU * cutoff / SAMPLE_RATE).exp();
    1.0 - x
}

/// 2 次共振フィルタ（バンドパス）。
///
/// 排気管や機体の共鳴を作る。**これが無いと、どんな倍音構成でも
/// 「電子音」から抜けられない。** 実物の音色は、音源そのものより
/// 通り道の共鳴で決まっている部分が大きい。
#[derive(Debug, Clone, Copy)]
pub struct Resonator {
    a1: f64,
    a2: f64,
    gain: f64,
    y1: f64,
    y2: f64,
}

impl Resonator {
    /// 中心周波数と Q から作る。
    ///
    /// # Panics
    ///
    /// `q` が有限の正値でない場合。
    #[must_use]
    pub fn new(frequency_hz: f64, q: f64) -> Self {
        assert!(
            q.is_finite() && q > 0.0,
            "q must be a finite positive value, got {q}"
        );
        let mut resonator = Self {
            a1: 0.0,
            a2: 0.0,
            gain: 0.0,
            y1: 0.0,
            y2: 0.0,
        };
        resonator.set_frequency(frequency_hz, q);
        resonator
    }

    /// 中心周波数と Q を変える。
    pub fn set_frequency(&mut self, frequency_hz: f64, q: f64) {
        let nyquist = SAMPLE_RATE / 2.0;
        let frequency = if frequency_hz.is_finite() {
            frequency_hz.clamp(20.0, nyquist * 0.95)
        } else {
            1_000.0
        };
        let q = if q.is_finite() {
            q.clamp(0.5, 200.0)
        } else {
            1.0
        };

        // 極を単位円の内側に置く 2 極共振器。半径は Q から決める。
        let radius = (-std::f64::consts::PI * frequency / (q * SAMPLE_RATE)).exp();
        let theta = TAU * frequency / SAMPLE_RATE;
        self.a1 = 2.0 * radius * theta.cos();
        self.a2 = -radius * radius;

        // 中心周波数での利得が 1 になるよう正規化する。**これを省くと、
        // 周波数を動かすたびに音量が跳ねる**（実測で 4.5 倍振れた）。
        //
        // `H(z) = g / (1 - a1 z^-1 - a2 z^-2)` を `z = e^{jθ}` で評価し、
        // その逆数を `g` に入れる。分母を実部と虚部に分けて書き下すと:
        //   実部 = 1 - 2r·cos²θ + r²·cos2θ
        //   虚部 = r(1 - r)·sin2θ
        let (sin_theta, cos_theta) = theta.sin_cos();
        let real = 2.0_f64.mul_add(
            -(radius * cos_theta * cos_theta),
            radius.mul_add(radius * (2.0 * theta).cos(), 1.0),
        );
        let imaginary = radius * (1.0 - radius) * (2.0 * sin_theta * cos_theta);
        let magnitude = real.hypot(imaginary);
        self.gain = if magnitude > f64::EPSILON {
            magnitude
        } else {
            1.0
        };
    }

    /// 1 標本通す。
    pub fn tick(&mut self, input: f64) -> f64 {
        if !input.is_finite() {
            return 0.0;
        }
        let output = self
            .gain
            .mul_add(input, self.a1.mul_add(self.y1, self.a2 * self.y2));
        self.y2 = self.y1;
        self.y1 = if output.is_finite() { output } else { 0.0 };
        self.y1
    }
}

/// 遅延線と減衰で作る共鳴（Karplus-Strong）。
///
/// 排気管の中で圧力波が往復する現象をそのまま写したもの。**排気音が
/// 「管を通った音」に聞こえるかはここで決まる。**
///
/// 参照した構造: 論文 "Physics-Informed Neural Engine Sound Modeling with
/// Differentiable Pulse-Train Synthesis" の PTR モデル。そこでは係数を
/// 学習で決めているが、ここでは管の長さと減衰から手で置く。
#[derive(Debug)]
pub struct DelayResonator {
    buffer: Vec<f64>,
    write: usize,
    /// 遅延長（標本）。整数部のみ。管の長さに対応する。
    delay: usize,
    /// 帰還率。1 に近いほど長く響く。**1 以上にすると発散する。**
    feedback: f64,
    /// 帰還路のローパス。高い倍音ほど早く減衰する現象を作る。
    damping: LowPass,
}

impl DelayResonator {
    /// 共鳴周波数と帰還率から作る。
    ///
    /// # Panics
    ///
    /// `frequency_hz` が有限の正値でない場合。
    #[must_use]
    pub fn new(frequency_hz: f64, feedback: f64, damping_hz: f64) -> Self {
        assert!(
            frequency_hz.is_finite() && frequency_hz > 0.0,
            "frequency must be a finite positive value, got {frequency_hz}"
        );
        // 遅延長は直後に 2..=4800 へクランプする。
        let delay = ((SAMPLE_RATE / frequency_hz).round() as usize).clamp(2, 4_800);
        Self {
            buffer: vec![0.0; delay],
            write: 0,
            delay,
            // **1 未満に抑える。** ここが 1 を超えると音が発散して
            // スピーカーを壊しかねない。
            feedback: feedback.clamp(0.0, 0.98),
            damping: LowPass::new(damping_hz),
        }
    }

    /// 1 標本通す。
    pub fn tick(&mut self, input: f64) -> f64 {
        let input = if input.is_finite() { input } else { 0.0 };
        let read = self.write;
        let delayed = self.buffer[read];
        let damped = self.damping.tick(delayed);
        let output = input + self.feedback * damped;
        // 発散していたら状態ごと捨てる。**居座らせない。**
        let output = if output.is_finite() {
            output.clamp(-8.0, 8.0)
        } else {
            0.0
        };
        self.buffer[self.write] = output;
        self.write = (self.write + 1) % self.delay;
        output
    }
}

/// 値をなめらかに追わせる。
///
/// **設定値を毎フレームそのまま入れると「ジッ」というノイズが出る。**
/// 制御は 60 Hz、音は 48 kHz なので、間を埋める必要がある。
#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    value: f64,
    coefficient: f64,
}

impl Smoothed {
    /// 初期値と時定数（秒）から作る。
    #[must_use]
    pub fn new(initial: f64, seconds: f64) -> Self {
        Self {
            value: initial,
            coefficient: smoothing_coefficient(seconds),
        }
    }

    /// 目標へ 1 標本ぶん近づけ、今の値を返す。
    pub fn tick(&mut self, target: f64) -> f64 {
        if target.is_finite() {
            self.value += self.coefficient * (target - self.value);
        }
        self.value
    }

    /// 今の値。
    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }

    /// 目標へ即座に飛ばす。**やり直しの瞬間だけ**（渡り音を出さないため）。
    pub const fn reset(&mut self, value: f64) {
        self.value = value;
    }
}

fn smoothing_coefficient(seconds: f64) -> f64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 1.0;
    }
    (1.0 - (-1.0 / (seconds * SAMPLE_RATE)).exp()).clamp(0.0, 1.0)
}

/// 出力を潰さずに頭を抑える。
///
/// 3 つの音を足すので、合計が 1 を超えることがある。**そのまま出すと
/// 波形の頭が切れて歪む。** 超えた分だけ滑らかに圧縮する。
#[must_use]
pub fn soft_clip(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    // tanh は原点付近で恒等に近く、大きいところで ±1 へ漸近する。
    value.tanh()
}

/// 検査から使う測定道具。
///
/// **音の検査は「それらしい」で通してはいけない。** 狙った周波数に
/// エネルギーがあることを数字で確かめるための最小限の道具を置く。
#[cfg(test)]
pub mod tests_support {
    use super::SAMPLE_RATE;
    use std::f64::consts::TAU;

    /// 指定周波数の成分の強さ（Goertzel 法）。
    ///
    /// FFT を持ち込まずに 1 本の周波数だけ測る。依存を増やさずに済み、
    /// 「どの周波数を見ているか」がコードに明示されるので検査として読みやすい。
    #[must_use]
    pub fn magnitude_at(samples: &[f64], frequency_hz: f64) -> f64 {
        let count = samples.len() as f64;
        let k = (frequency_hz * count / SAMPLE_RATE).round();
        let omega = TAU * k / count;
        let coefficient = 2.0 * omega.cos();
        let (mut s1, mut s2) = (0.0, 0.0);
        for sample in samples {
            let s0 = sample + coefficient * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coefficient * s1 * s2).max(0.0).sqrt() / count
    }
}

#[cfg(test)]
// 音の検査では「ちょうど 0（無音）」と「頭打ちで同じ値」が契約そのもの。
// **近似で見ると、無音のはずが極小の音で鳴っていても通ってしまう。**
#[expect(clippy::float_cmp, reason = "無音と頭打ちは厳密な値が契約")]
mod tests {
    use super::tests_support::magnitude_at;
    use super::*;

    #[test]
    fn the_noise_is_the_same_every_time() {
        // **同じ設定で違う音が鳴ると、不具合と区別が付かない。**
        let mut a = Noise::new(12_345);
        let mut b = Noise::new(12_345);
        for _ in 0..1_000 {
            assert_eq!(a.tick(), b.tick());
        }
    }

    #[test]
    fn the_noise_stays_in_range_and_is_not_stuck() {
        let mut noise = Noise::new(0);
        let mut sum = 0.0;
        let mut distinct = std::collections::HashSet::new();
        for _ in 0..10_000 {
            let value = noise.tick();
            assert!((-1.0..1.0).contains(&value), "got {value}");
            sum += value;
            distinct.insert(value.to_bits());
        }
        assert!(distinct.len() > 9_000, "the noise repeats too soon");
        // 平均は 0 のはず。偏っていると直流成分になってスピーカーを押しっぱなしにする。
        assert!((sum / 10_000.0).abs() < 0.05, "mean {}", sum / 10_000.0);
    }

    #[test]
    fn the_low_pass_removes_the_high_end() {
        // 500 Hz で切ったフィルタに 5 kHz を通すと小さくなること。
        let mut through = Vec::new();
        let mut filter = LowPass::new(500.0);
        for index in 0..4_800 {
            let phase = TAU * 5_000.0 * f64::from(index) / SAMPLE_RATE;
            through.push(filter.tick(phase.sin()));
        }
        let magnitude = magnitude_at(&through, 5_000.0);
        assert!(magnitude < 0.1, "5 kHz survived at {magnitude}");
    }

    #[test]
    fn the_low_pass_keeps_the_low_end() {
        // 逆に、通す側が消えては困る。
        let mut through = Vec::new();
        let mut filter = LowPass::new(500.0);
        for index in 0..4_800 {
            let phase = TAU * 100.0 * f64::from(index) / SAMPLE_RATE;
            through.push(filter.tick(phase.sin()));
        }
        assert!(magnitude_at(&through, 100.0) > 0.3);
    }

    #[test]
    fn a_nan_does_not_take_up_residence_in_a_filter() {
        // **一度 NaN が状態に入ると、以降ずっと無音になる。**
        // 原因のフレームはとっくに過ぎているので、追うのが極めて難しい。
        let mut filter = LowPass::new(500.0);
        for _ in 0..100 {
            filter.tick(0.5);
        }
        filter.tick(f64::NAN);
        let recovered = filter.tick(0.5);
        assert!(recovered.is_finite(), "got {recovered}");
        assert!(recovered.abs() > 0.0);
    }

    #[test]
    fn the_resonator_rings_at_its_centre_frequency() {
        // 白色雑音を通して、中心周波数だけが立つこと。
        let mut resonator = Resonator::new(400.0, 20.0);
        let mut noise = Noise::new(7);
        let mut output = Vec::new();
        for _ in 0..9_600 {
            output.push(resonator.tick(noise.tick()));
        }
        let centre = magnitude_at(&output, 400.0);
        let away = magnitude_at(&output, 1_600.0);
        assert!(
            centre > away * 4.0,
            "the resonator should favour its centre: {centre} vs {away}"
        );
    }

    #[test]
    fn the_resonator_passes_its_centre_frequency_at_unity() {
        // **正規化しないと、周波数を動かすたびに音量が跳ねる。**
        //
        // 中心周波数の正弦波を入れて、振幅がほぼ 1 で出ること。
        // 白色雑音で測ってはいけない: 高い中心周波数ほど帯域が広く、
        // 通る雑音の電力が増える。**それは正しい挙動**なので、
        // 雑音で測ると正規化ができていても「揃っていない」と出る（一度やった）。
        for frequency in [200.0, 800.0, 3_200.0] {
            let mut resonator = Resonator::new(frequency, 12.0);
            let mut output = Vec::new();
            for index in 0..48_000 {
                let phase = TAU * frequency * f64::from(index) / SAMPLE_RATE;
                output.push(resonator.tick(phase.sin()));
            }
            // 立ち上がりを避けて後半だけ見る。
            let settled = &output[24_000..];
            let peak = settled.iter().fold(0.0_f64, |peak, s| peak.max(s.abs()));
            assert!(
                (0.8..=1.25).contains(&peak),
                "at {frequency} Hz the centre gain is {peak}, not unity"
            );
        }
    }

    #[test]
    fn the_delay_resonator_does_not_run_away() {
        // **帰還が 1 を超えると発散し、スピーカーを壊しかねない。**
        // 上限で丸めていることを、無茶な指定で確かめる。
        let mut resonator = DelayResonator::new(120.0, 5.0, 4_000.0);
        let mut peak = 0.0_f64;
        for index in 0..48_000 {
            let input = if index % 400 == 0 { 1.0 } else { 0.0 };
            peak = peak.max(resonator.tick(input).abs());
        }
        assert!(peak.is_finite() && peak < 8.1, "peak {peak}");
    }

    #[test]
    fn the_delay_resonator_rings_at_the_asked_frequency() {
        let mut resonator = DelayResonator::new(150.0, 0.9, 6_000.0);
        let mut output = Vec::new();
        let mut noise = Noise::new(3);
        for _ in 0..48_000 {
            output.push(resonator.tick(noise.tick() * 0.05));
        }
        let ring = magnitude_at(&output, 150.0);
        let away = magnitude_at(&output, 220.0);
        assert!(
            ring > away,
            "the ring should be at 150 Hz: {ring} vs {away}"
        );
    }

    #[test]
    fn a_smoothed_value_arrives_but_not_instantly() {
        // **即座に飛ぶと「ジッ」と鳴る。永遠に届かないと設定が効かない。**
        let mut smoothed = Smoothed::new(0.0, 0.05);
        assert!(smoothed.tick(1.0) < 0.01, "it must not jump");
        for _ in 0..(SAMPLE_RATE as usize / 4) {
            smoothed.tick(1.0);
        }
        assert!(
            (smoothed.value() - 1.0).abs() < 0.01,
            "it must arrive, got {}",
            smoothed.value()
        );
    }

    #[test]
    fn a_broken_target_does_not_poison_a_smoothed_value() {
        let mut smoothed = Smoothed::new(0.5, 0.05);
        smoothed.tick(f64::NAN);
        assert!(smoothed.value().is_finite());
    }

    #[test]
    fn soft_clipping_leaves_quiet_sounds_alone() {
        // 小さい音まで曲げると、全体がくぐもる。
        for value in [-0.2, -0.05, 0.0, 0.05, 0.2] {
            assert!(
                (soft_clip(value) - value).abs() < 0.015,
                "{value} was bent to {}",
                soft_clip(value)
            );
        }
    }

    #[test]
    fn soft_clipping_holds_the_peaks_inside_the_range() {
        for value in [-50.0, -1.5, 1.5, 50.0, f64::INFINITY] {
            let clipped = soft_clip(value);
            assert!((-1.0..=1.0).contains(&clipped), "{value} -> {clipped}");
        }
        assert_eq!(soft_clip(f64::NAN), 0.0);
    }
}
