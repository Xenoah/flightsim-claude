//! 音を波形として作る。
//!
//! # なぜ合成するのか
//!
//! **音声ファイルを同梱すると、出所と再配布条件の判断が要る。** それは
//! 追加機体やコックピット内装が止まっているのと同じ理由で、実装とは別の
//! 判断待ちになる（[ADR-0003] のオープンデータ方針）。合成なら誰の権利にも
//! 触らず、生成が決定論的で、検査もできる。
//!
//! 実機を録音した音には敵わない。**「本物らしい音」を目指していない。**
//! 目指しているのは、計器を見なくても機体の状態が分かること。
//!
//! # 何を作るか
//!
//! | 音 | 作り方 | 何を伝えるか |
//! |---|---|---|
//! | エンジン | 基音 + 倍音 | 出力。ピッチと音量が回転数に連動する |
//! | 風切り | 帯域制限したノイズ | 対気速度。速いほど大きい |
//! | 失速警報 | 単音 | 失速が近いこと。**視覚に頼らず分かる唯一の手段** |
//!
//! # 継ぎ目の無いループ
//!
//! 生成した波形はループ再生する。**波形の終わりと始まりが繋がっていないと、
//! 1 周ごとにプチッと鳴る。** 周期の整数倍ちょうどの長さで作ることでこれを
//! 避ける。検査で固定してある（`the_loop_joins_without_a_click`）。
//!
//! [ADR-0003]: https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0003-terrain-data-source.md

// 標本数と標本の位置は [`MAX_SAMPLES`]（441,000）以下に抑えてある。
// f64 の仮数 52 bit にも usize にも余裕で収まるので、この 2 つの lint は
// ここでは実害を指さない。**上限を上げるときは、この前提も見直すこと。**
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "標本数は MAX_SAMPLES 以下。f64 の仮数にも usize にも収まる"
)]

/// 標本化周波数 `Hz`。
///
/// 44.1 kHz。ナイキスト周波数は 22 kHz で、人の可聴域を覆う。
/// 上げても聞こえる範囲は変わらず、生成した波形の容量だけ増える。
pub const SAMPLE_RATE: u32 = 44_100;

/// 生成する波形の最大の長さ（標本数）。
///
/// 44.1 kHz で 10 秒。**壊れた引数で巨大な確保をしないための線。**
pub const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 10;

/// 16 bit PCM の最大振幅。
const FULL_SCALE: f64 = 32_767.0;

/// 波形が取りうる振幅の上限。
///
/// 1.0 まで振らせない。**複数の音を同時に鳴らすので、余裕を残さないと
/// 混ざったところで頭が潰れる。**
const HEADROOM: f64 = 0.7;

/// 1 周期ぶんの標本数と、それに合う実際の周波数。
///
/// 指定した周波数ちょうどでは周期が標本の整数倍にならないので、
/// **整数倍になる一番近い周波数へ寄せる。** ずれは可聴域では分からない
/// （100 Hz の音で最大 0.05 Hz 程度）が、継ぎ目の有無は分かる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopLength {
    /// 波形の長さ（標本数）。
    pub samples: usize,
    /// 実際に鳴る周波数 `Hz`。指定値から少しずれる。
    pub frequency: f64,
}

impl LoopLength {
    /// `frequency` 付近で、`min_seconds` 以上の長さになる継ぎ目の無いループ長。
    ///
    /// # Panics
    ///
    /// `frequency` が有限の正値でない場合、`min_seconds` が有限でない場合。
    #[must_use]
    pub fn nearest(frequency: f64, min_seconds: f64) -> Self {
        assert!(
            frequency.is_finite() && frequency > 0.0,
            "frequency must be a finite positive value, got {frequency}"
        );
        assert!(
            min_seconds.is_finite() && min_seconds > 0.0,
            "min_seconds must be a finite positive value, got {min_seconds}"
        );
        let sample_rate = f64::from(SAMPLE_RATE);
        // 最低長を満たす周期数へ切り上げる。
        let periods = (frequency * min_seconds).ceil().max(1.0);
        let samples = (sample_rate * periods / frequency).round().max(1.0);
        let samples = (samples as usize).min(MAX_SAMPLES);
        Self {
            // 標本数を決めてから周波数を引き直す。**こちらが実際に鳴る値。**
            frequency: sample_rate * periods / (samples as f64),
            samples,
        }
    }
}

/// 決定論的な擬似乱数。
///
/// **`rand` を持ち込まない。** 音は毎回同じであってほしい（リプレイと、
/// 「さっきと違う音がした」という誤解を避けるため）。xorshift64。
#[derive(Debug, Clone, Copy)]
struct Noise(u64);

impl Noise {
    const fn new(seed: u64) -> Self {
        // 0 を種にすると以降ずっと 0 になる。
        Self(if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        })
    }

    /// 次の値を `[-1, 1)` で返す。
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // 上位 32 bit を使う。下位は周期が短い。
        let unit = f64::from((self.0 >> 32) as u32) / f64::from(u32::MAX);
        unit.mul_add(2.0, -1.0)
    }
}

/// エンジン音の波形。
///
/// 基音にプロペラの羽根通過音（2 倍音）と機械音（3・4 倍音）を重ねる。
/// **実機の録音とは似ていない。** 出力の増減が耳で分かることが目的。
///
/// `blade_passage_hz` は羽根通過周波数。2 枚羽根・2400 rpm なら
/// 2400/60 × 2 = 80 Hz。再生速度で上下させるので、ここは基準の 1 点だけ作る。
#[must_use]
pub fn engine(blade_passage_hz: f64) -> Vec<i16> {
    let loop_length = LoopLength::nearest(blade_passage_hz, 0.5);
    let mut samples = Vec::with_capacity(loop_length.samples);
    let step = std::f64::consts::TAU * loop_length.frequency / f64::from(SAMPLE_RATE);
    // 倍音の重み。上の倍音ほど弱くする。**同じ重みにすると金属的で耳障りになる。**
    let harmonics = [(1.0, 1.0), (2.0, 0.55), (3.0, 0.28), (4.0, 0.14)];
    let normaliser: f64 = harmonics.iter().map(|(_, weight)| weight).sum();

    for index in 0..loop_length.samples {
        let phase = step * index as f64;
        let value: f64 = harmonics
            .iter()
            .map(|(multiple, weight)| (phase * multiple).sin() * weight)
            .sum();
        samples.push(to_pcm(value / normaliser));
    }
    samples
}

/// 風切り音の波形。
///
/// 帯域制限したノイズ。**生のホワイトノイズは「砂嵐」に聞こえて風にならない**
/// ので、1 次のローパスで高域を落とす。
///
/// ループの継ぎ目はノイズには作れない（周期が無い）ので、末尾を先頭へ
/// クロスフェードして繋ぐ。
#[must_use]
pub fn wind(seconds: f64) -> Vec<i16> {
    assert!(
        seconds.is_finite() && seconds > 0.0,
        "seconds must be a finite positive value, got {seconds}"
    );
    let count = ((f64::from(SAMPLE_RATE) * seconds).round() as usize).clamp(2, MAX_SAMPLES);
    // クロスフェードに使う長さ。全体の 1/8。
    let blend = count / 8;

    let mut noise = Noise::new(0x5eed_1234_5678_9abc);
    let mut low_passed = Vec::with_capacity(count + blend);
    let mut previous = 0.0;
    // 1 次ローパス。係数はカットオフを決める。小さいほど低い音になる。
    const SMOOTHING: f64 = 0.08;
    for _ in 0..count + blend {
        previous += SMOOTHING * (noise.next() - previous);
        low_passed.push(previous);
    }

    // ローパスで振幅が下がるので、最大値で正規化してから使う。
    let peak = low_passed
        .iter()
        .fold(0.0_f64, |peak, value| peak.max(value.abs()))
        .max(f64::MIN_POSITIVE);

    // **先頭に、末尾の続きを混ぜ込む。**
    //
    // 出力するのは `n[0..count]`。ループでは `n[count-1]` の次に `n[0]` が来る
    // ので、そこが繋がっていないと 1 周ごとに鳴る。そこで先頭 `blend` 標本を
    // 「`n[count..]`（＝ `n[count-1]` の続き）」へ寄せる。先頭ちょうどは
    // 完全に続きの値になるので、継ぎ目が消える。
    //
    // 末尾側を混ぜる形にすると、混ぜ先が先頭ではなく「その先」になり、
    // 繋がらない（一度そう書いて、継ぎ目が 2.5 倍残った）。
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let value = low_passed[index] / peak;
        let value = if index < blend {
            let position = index as f64 / blend as f64;
            let continuation = low_passed[count + index] / peak;
            value.mul_add(position, continuation * (1.0 - position))
        } else {
            value
        };
        samples.push(to_pcm(value));
    }
    samples
}

/// 失速警報の波形。
///
/// 実機の失速警報（リード式のホーン）に倣った単音。少し歪ませて、
/// **エンジン音の中でも埋もれない**ようにする。
#[must_use]
pub fn stall_horn(frequency: f64) -> Vec<i16> {
    let loop_length = LoopLength::nearest(frequency, 0.25);
    let mut samples = Vec::with_capacity(loop_length.samples);
    let step = std::f64::consts::TAU * loop_length.frequency / f64::from(SAMPLE_RATE);
    for index in 0..loop_length.samples {
        let phase = step * index as f64;
        // 基音 + 3 倍音。矩形波に近づけて「鳴っている」感じを出す。
        let value = (phase.sin()).mul_add(0.8, (phase * 3.0).sin() * 0.2);
        samples.push(to_pcm(value));
    }
    samples
}

/// `[-1, 1]` の値を 16 bit PCM にする。範囲外は頭打ち。
fn to_pcm(value: f64) -> i16 {
    if !value.is_finite() {
        return 0;
    }
    let scaled = (value * HEADROOM * FULL_SCALE).clamp(-FULL_SCALE, FULL_SCALE);
    // clamp 済みなので i16 に収まる。
    #[expect(
        clippy::cast_possible_truncation,
        reason = "上で ±32767 にクランプしてある"
    )]
    let value = scaled as i16;
    value
}

/// 16 bit PCM モノラルの標本列を WAV にする。
///
/// **rodio が読める形にするためだけの薄い変換。** 独自形式を足さない。
///
/// # Panics
///
/// 標本数が `u32` のバイト長に収まらない場合。[`MAX_SAMPLES`] を守って
/// いれば起きない。
#[must_use]
pub fn to_wav(samples: &[i16]) -> Vec<u8> {
    let data_bytes = u32::try_from(samples.len() * 2).expect("the sample count fits in a u32");
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);

    wav.extend_from_slice(b"RIFF");
    // ファイル全体から先頭 8 バイトを引いた長さ。
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes()); // PCM の fmt チャンクは 16 バイト
    wav.extend_from_slice(&1_u16.to_le_bytes()); // 1 = 非圧縮 PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // モノラル
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // 1 秒あたりのバイト数
    wav.extend_from_slice(&2_u16.to_le_bytes()); // 1 標本あたりのバイト数
    wav.extend_from_slice(&16_u16.to_le_bytes()); // 量子化 bit 数

    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ループの継ぎ目の段差。最大振幅に対する割合で返す。
    fn seam_step(samples: &[i16]) -> f64 {
        let first = f64::from(samples[0]);
        let last = f64::from(samples[samples.len() - 1]);
        // 隣り合う標本どうしの平均的な段差と比べる。
        let typical = samples
            .windows(2)
            .map(|pair| (f64::from(pair[1]) - f64::from(pair[0])).abs())
            .fold(0.0_f64, f64::max)
            .max(1.0);
        (first - last).abs() / typical
    }

    #[test]
    fn the_loop_joins_without_a_click() {
        // **継ぎ目が段差になっていると 1 周ごとにプチッと鳴る。**
        // 波形の中で最大の段差より小さければ、そこだけ目立つことはない。
        // 実測: エンジン 1.0、ホーン 0.99、風 0.4 前後。
        // **2 を超えるのは繋がっていない証拠**（クロスフェードを
        // 逆向きに書いたときは 2.48 だった）。
        for (name, samples) in [
            ("engine", engine(80.0)),
            ("horn", stall_horn(800.0)),
            ("wind", wind(1.0)),
        ] {
            let step = seam_step(&samples);
            assert!(
                step <= 1.2,
                "the {name} loop seam is a jump of {step} typical steps"
            );
        }
    }

    #[test]
    fn the_loop_length_holds_a_whole_number_of_periods() {
        let loop_length = LoopLength::nearest(80.0, 0.5);
        let periods = loop_length.frequency * loop_length.samples as f64 / f64::from(SAMPLE_RATE);
        assert!(
            (periods - periods.round()).abs() < 1e-9,
            "got {periods} periods, which does not join up"
        );
        assert!(loop_length.samples as f64 / f64::from(SAMPLE_RATE) >= 0.5);
    }

    #[test]
    fn the_actual_frequency_stays_close_to_the_asked_one() {
        // 整数周期へ寄せるためにずれるが、**耳で分かるほどずらさない。**
        for asked in [40.0, 80.0, 400.0, 800.0, 2_000.0] {
            let loop_length = LoopLength::nearest(asked, 0.5);
            let error = (loop_length.frequency - asked).abs();
            assert!(
                error < 0.1,
                "asked for {asked} Hz, got {} Hz",
                loop_length.frequency
            );
        }
    }

    #[test]
    fn every_waveform_leaves_headroom() {
        // **複数の音を同時に鳴らす。** 単体で振り切っていると混ざって割れる。
        for samples in [engine(80.0), stall_horn(800.0), wind(1.0)] {
            let peak = samples
                .iter()
                .map(|sample| i32::from(*sample).abs())
                .max()
                .expect("the waveform is not empty");
            assert!(
                peak <= 23_000,
                "peak {peak} leaves no room for mixing (full scale is 32767)"
            );
            assert!(peak > 5_000, "peak {peak} is too quiet to hear");
        }
    }

    #[test]
    fn the_waveforms_are_the_same_every_time() {
        // **同じ設定で違う音が鳴ると、不具合と区別が付かない。**
        // 風はノイズなので、ここが決定論的であることが特に効く。
        assert_eq!(wind(0.5), wind(0.5));
        assert_eq!(engine(80.0), engine(80.0));
    }

    #[test]
    fn the_wind_is_not_a_constant_tone() {
        // ノイズが潰れて直流や単音になっていないこと。
        let samples = wind(1.0);
        let distinct = samples.iter().collect::<std::collections::HashSet<_>>();
        assert!(
            distinct.len() > 1_000,
            "the wind collapsed to {} distinct values",
            distinct.len()
        );
    }

    #[test]
    fn a_wav_header_says_what_the_data_is() {
        // rodio が読めないと**無音になるだけで、エラーは出ない。**
        let wav = to_wav(&engine(80.0));
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");

        let declared = u32::from_le_bytes(wav[40..44].try_into().expect("4 bytes"));
        assert_eq!(
            declared as usize,
            wav.len() - 44,
            "the declared data length must match what follows"
        );
        let riff = u32::from_le_bytes(wav[4..8].try_into().expect("4 bytes"));
        assert_eq!(riff as usize, wav.len() - 8);

        let channels = u16::from_le_bytes(wav[22..24].try_into().expect("2 bytes"));
        let rate = u32::from_le_bytes(wav[24..28].try_into().expect("4 bytes"));
        let bits = u16::from_le_bytes(wav[34..36].try_into().expect("2 bytes"));
        assert_eq!(channels, 1);
        assert_eq!(rate, SAMPLE_RATE);
        assert_eq!(bits, 16);
    }

    #[test]
    fn an_empty_waveform_still_makes_a_valid_wav() {
        let wav = to_wav(&[]);
        assert_eq!(wav.len(), 44);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().expect("4")), 0);
    }

    #[test]
    fn out_of_range_and_broken_values_do_not_wrap_around() {
        // **クランプを忘れると、大きすぎる値が反対の極性で鳴る。**
        // 一番大きな音が一番耳障りな形で壊れる。
        assert!(
            to_pcm(10.0) > 0,
            "a large positive value must stay positive"
        );
        assert!(
            to_pcm(-10.0) < 0,
            "a large negative value must stay negative"
        );
        assert_eq!(to_pcm(f64::NAN), 0);
        assert_eq!(to_pcm(f64::INFINITY), 0);
    }

    #[test]
    fn the_generated_waveforms_stay_within_the_size_limit() {
        assert!(engine(80.0).len() <= MAX_SAMPLES);
        // 上限を超える長さを頼んでも、そこで止まること。
        assert!(wind(3_600.0).len() <= MAX_SAMPLES);
    }

    #[test]
    fn a_very_low_frequency_does_not_ask_for_an_endless_buffer() {
        // 0 に近い周波数で 1 周期を作ろうとすると巨大な確保になる。
        let loop_length = LoopLength::nearest(0.001, 0.5);
        assert!(loop_length.samples <= MAX_SAMPLES);
    }
}
