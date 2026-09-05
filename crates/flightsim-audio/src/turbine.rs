//! ターボファン（戦闘機）の音。
//!
//! # ピストンエンジンと何が違うのか
//!
//! ピストンは**離散した爆発の連なり**で、点火周波数（せいぜい 90 Hz）と
//! その倍音が音の骨格になる。タービンは**連続燃焼で、回転する翼列**が
//! 音を作る。骨格になる周波数が 2 桁違う。
//!
//! | 音源 | 周波数 | 由来 |
//! |---|---|---|
//! | ファンの翼通過音 | `羽根数 × N1/60` | **これが「キーン」の正体。** 数 kHz |
//! | バズソー | `N1/60` の倍音 | **翼端が超音速になったとき**だけ出る |
//! | 圧縮機の翼通過音 | `羽根数 × N2/60` | さらに高い。N2 は N1 より速く回る |
//! | 排気の混合音 | ピークは `0.2 × 排気速度 / ノズル径` | 広帯域の「ゴー」。数百 Hz |
//! | アフターバーナー | 広帯域 + 低い唸り | 点火すると全体が一段大きくなる |
//!
//! ## バズソーが要点
//!
//! ファンの翼端が音速を超えると、各翼から出る衝撃波が前方へ抜ける。
//! 翼の製造ばらつきで 1 枚ずつ強さが違うため、**翼通過周波数ではなく
//! 軸回転周波数の倍音**にエネルギーが並ぶ。あの独特の濁った「ガーッ」は
//! これで、翼通過音の澄んだ「キーン」とは別物である。
//!
//! **両方要る。** 翼通過音だけだと電子的な単音になり、バズソーだけだと
//! 濁って何の音か分からなくなる。
//!
//! ## 排気の混合音
//!
//! 乱流混合が作る広帯域音。ピークは Strouhal 数 0.2 の関係
//! `f = 0.2 × u / d` で決まる。ノズル径 0.6 m・排気 600 m/s なら約 200 Hz。
//! 強度は加熱ジェットで**速度の 6 乗**に比例するので、出力を上げると
//! 音量が跳ね上がる。
//!
//! # 諸元の出どころ
//!
//! - F404: 3 段ファン、N1 は設計点で 13,270 rpm、N2 は 16,810 rpm
//! - F110-129: 3 段ファン、第 1 段は **32 枚**
//!
//! **ノズル径・排気速度・アイドル回転数は公表値ではない。** 軽戦闘機として
//! 妥当な範囲の代表値を置いたもので、特定の実機の性能を写していない。

use std::f64::consts::TAU;

use crate::dsp::{HighPass, LowPass, Noise, Resonator, SAMPLE_RATE, Smoothed};

/// ターボファンの諸元。
///
/// **音のためだけの諸元。** FDM は回転数も排気速度も持たない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbineSpec {
    /// ファン第 1 段の羽根数。翼通過音の高さを決める。
    pub fan_blades: u32,
    /// 高圧圧縮機の 1 段あたりの羽根数。さらに高い音を出す。
    pub compressor_blades: u32,
    /// ファンの直径 `m`。翼端速度から、バズソーが出るかを決める。
    pub fan_diameter: f64,
    /// アイドルの N1 `rpm`。
    pub idle_n1: f64,
    /// 最大の N1 `rpm`。
    pub max_n1: f64,
    /// N2 / N1 の比。高圧軸は低圧軸より速く回る。
    pub spool_ratio: f64,
    /// ノズル径 `m`。排気音のピーク周波数を決める。
    pub nozzle_diameter: f64,
    /// 軍用推力（アフターバーナー無し）での排気速度 `m/s`。
    pub military_exhaust_speed: f64,
    /// アフターバーナー全開での排気速度 `m/s`。
    pub afterburner_exhaust_speed: f64,
    /// アフターバーナーが点く出力。これを超えると点火する。
    pub afterburner_threshold: f64,
}

impl Default for TurbineSpec {
    /// 軽戦闘機の低バイパス比ターボファン（F404 / F110 相当）。
    ///
    /// **公表値と、そうでない値が混ざっている。**
    ///
    /// | 値 | 出どころ |
    /// |---|---|
    /// | ファン 32 枚 | F110-129 第 1 段の公表値 |
    /// | 最大 N1 13,270 rpm | F404 の設計点 |
    /// | N2/N1 = 1.27 | F404 の 16,810 / 13,270 |
    /// | それ以外 | **代表値。特定の実機を写していない** |
    fn default() -> Self {
        Self {
            fan_blades: 32,
            // 高圧圧縮機は段あたりの翼が多い。9 段の前段側として置く。
            compressor_blades: 44,
            fan_diameter: 0.89,
            // 地上アイドルは最大の 6 割ほど。**タービンはアイドルでも高速で回る。**
            idle_n1: 8_000.0,
            max_n1: 13_270.0,
            spool_ratio: 16_810.0 / 13_270.0,
            nozzle_diameter: 0.60,
            military_exhaust_speed: 600.0,
            afterburner_exhaust_speed: 900.0,
            afterburner_threshold: 0.90,
        }
    }
}

impl TurbineSpec {
    /// ファンの翼通過周波数 `Hz`。**「キーン」の高さ。**
    #[must_use]
    pub fn fan_blade_passage_hz(self, n1_rpm: f64) -> f64 {
        n1_rpm / 60.0 * f64::from(self.fan_blades)
    }

    /// 低圧軸の回転周波数 `Hz`。バズソーの基本周波数。
    #[must_use]
    pub fn shaft_hz(self, n1_rpm: f64) -> f64 {
        n1_rpm / 60.0
    }

    /// 高圧圧縮機の翼通過周波数 `Hz`。
    #[must_use]
    pub fn compressor_blade_passage_hz(self, n1_rpm: f64) -> f64 {
        n1_rpm * self.spool_ratio / 60.0 * f64::from(self.compressor_blades)
    }

    /// ファン翼端のマッハ数。
    ///
    /// **1 を超えるとバズソーが出る。** 翼端から出た衝撃波が前方へ抜ける
    /// ようになるため。音速は海面標準の 340 m/s で固定する（高度による
    /// 変化は音の印象を左右しない）。
    #[must_use]
    pub fn tip_mach(self, n1_rpm: f64) -> f64 {
        let tip_speed = std::f64::consts::PI * self.fan_diameter * n1_rpm / 60.0;
        tip_speed / 340.0
    }

    /// 排気速度 `m/s`。出力とアフターバーナーで決まる。
    #[must_use]
    pub fn exhaust_speed(self, throttle: f64) -> f64 {
        let throttle = throttle.clamp(0.0, 1.0);
        // アイドルでも排気は出ている。**ここを低くしすぎると、
        // アイドルが純粋なファンの笛になる**（実測でアイドルの
        // 全エネルギーの 97% が 4 kHz 以上に寄った）。
        // 軍用推力までは出力に比例させる。
        let military = self
            .military_exhaust_speed
            .mul_add(throttle * 0.60, self.military_exhaust_speed * 0.40);
        if throttle <= self.afterburner_threshold {
            return military;
        }
        // アフターバーナー域。残りの出力で軍用から最大まで持ち上げる。
        let span = (1.0 - self.afterburner_threshold).max(1e-6);
        let position = ((throttle - self.afterburner_threshold) / span).clamp(0.0, 1.0);
        military + (self.afterburner_exhaust_speed - self.military_exhaust_speed) * position
    }

    /// 排気の混合音のピーク周波数 `Hz`。
    ///
    /// Strouhal 数 0.2 の関係 `f = 0.2 × u / d`。
    #[must_use]
    pub fn jet_peak_hz(self, throttle: f64) -> f64 {
        0.2 * self.exhaust_speed(throttle) / self.nozzle_diameter.max(0.05)
    }

    /// アフターバーナーが点いている度合い `[0, 1]`。
    #[must_use]
    pub fn afterburner_fraction(self, throttle: f64) -> f64 {
        let throttle = throttle.clamp(0.0, 1.0);
        if throttle <= self.afterburner_threshold {
            return 0.0;
        }
        let span = (1.0 - self.afterburner_threshold).max(1e-6);
        ((throttle - self.afterburner_threshold) / span).clamp(0.0, 1.0)
    }
}

/// 出力から N1 を見積もる。
///
/// # これは推定であって、物理ではない
///
/// **FDM は回転数を持っていない。** 音のためだけの式である。
/// **飛び方には一切影響しない。**
///
/// タービンはプロペラと違い、対気速度で回転が変わらない（自分で吸い込む量を
/// 決めるので、負荷が抜けるという現象が無い）。だから引数は出力だけ。
///
/// 出力に対して線形ではなく、**低出力側で回転が動きやすい**曲線にしてある。
/// 実機のスロットルも下の方が回転の変化が大きい。
#[must_use]
pub fn estimate_n1(spec: TurbineSpec, throttle: f64) -> f64 {
    let throttle = if throttle.is_finite() {
        throttle.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let span = (spec.max_n1 - spec.idle_n1).max(0.0);
    spec.idle_n1 + span * throttle.powf(0.75)
}

/// 上りと下りで速さが違う追従。
///
/// **タービンは回転が上がるのが遅い。** アイドルから軍用推力まで数秒かかる
/// のが特徴で、そこを即座に追わせると別の乗り物の音になる。
/// 落ちる方はもう少し速い。
#[derive(Debug, Clone, Copy)]
struct SpoolLag {
    value: f64,
    rising: Smoothed,
    falling: Smoothed,
}

impl SpoolLag {
    fn new(initial: f64, rise_seconds: f64, fall_seconds: f64) -> Self {
        Self {
            value: initial,
            rising: Smoothed::new(initial, rise_seconds),
            falling: Smoothed::new(initial, fall_seconds),
        }
    }

    fn tick(&mut self, target: f64) -> f64 {
        // 両方に今の値を入れてから、向きに応じて片方の結果を採る。
        // **片方だけ更新すると、向きが変わった瞬間に値が飛ぶ。**
        self.rising.reset(self.value);
        self.falling.reset(self.value);
        self.value = if target > self.value {
            self.rising.tick(target)
        } else {
            self.falling.tick(target)
        };
        self.value
    }

    const fn reset(&mut self, value: f64) {
        self.value = value;
    }

    const fn value(self) -> f64 {
        self.value
    }
}

/// ターボファンの音を 1 標本ずつ作る。
#[derive(Debug)]
pub struct TurbineVoice {
    spec: TurbineSpec,

    /// ファンの翼通過位相。
    fan_phase: f64,
    /// 低圧軸の回転位相。バズソーに使う。
    shaft_phase: f64,
    /// 高圧圧縮機の翼通過位相。
    compressor_phase: f64,

    /// バズソーの翼ごとのばらつき。**製造公差を模す。**
    /// これが無いと軸回転の倍音が出ず、澄んだ単音のままになる。
    blade_gains: Vec<f64>,

    /// 排気の混合音。
    jet_noise: Noise,
    jet_filter: Resonator,
    jet_body: LowPass,
    /// アフターバーナーの低い唸り。
    burner_noise: Noise,
    burner_filter: LowPass,
    /// 吸気の広帯域。
    intake_noise: Noise,
    intake_filter: Resonator,
    /// 直流を切る。
    dc_blocker: HighPass,

    /// 回転の追従。**上りが遅い。**
    n1: SpoolLag,
    /// 出力の追従。排気の勢いはもう少し速く追う。
    throttle: Smoothed,
}

impl TurbineVoice {
    /// 諸元を決めて作る。
    #[must_use]
    pub fn new(spec: TurbineSpec) -> Self {
        let count = spec.fan_blades.max(1);
        // 翼ごとのわずかな違い。決定論的に作る（乱数を持ち込まない）。
        let blade_gains = (0..count)
            .map(|index| {
                let phase = f64::from(index) * 2.399_963; // 黄金角。並びに周期を作らない
                1.0 + phase.sin() * 0.35 + (phase * 2.7).sin() * 0.18
            })
            .collect();

        Self {
            spec,
            fan_phase: 0.0,
            shaft_phase: 0.0,
            compressor_phase: 0.0,
            blade_gains,
            jet_noise: Noise::new(0x1234_5678_9abc_def0),
            jet_filter: Resonator::new(200.0, 1.1),
            jet_body: LowPass::new(3_000.0),
            burner_noise: Noise::new(0xfeed_face_dead_beef),
            burner_filter: LowPass::new(220.0),
            intake_noise: Noise::new(0x0bad_c0de_1337_9999),
            intake_filter: Resonator::new(2_600.0, 1.4),
            dc_blocker: HighPass::new(40.0),
            // **アイドルから軍用まで数秒。** ここを速くすると別の乗り物になる。
            n1: SpoolLag::new(spec.idle_n1, 2.6, 1.6),
            throttle: Smoothed::new(0.0, 0.35),
        }
    }

    /// 諸元。
    #[must_use]
    pub const fn spec(&self) -> TurbineSpec {
        self.spec
    }

    /// 今なぞっている N1。
    #[must_use]
    pub const fn n1(&self) -> f64 {
        self.n1.value()
    }

    /// 開始時の値へ飛ばす。やり直しの瞬間に使う。
    pub const fn reset(&mut self, n1: f64, throttle: f64) {
        self.n1.reset(n1);
        self.throttle.reset(throttle);
    }

    /// 1 標本作る。
    pub fn tick(&mut self, target_n1: f64, throttle: f64) -> f64 {
        let n1 = self.n1.tick(target_n1).max(0.0);
        let throttle = self.throttle.tick(throttle).clamp(0.0, 1.0);

        let shaft_hz = self.spec.shaft_hz(n1);
        let fan_hz = self.spec.fan_blade_passage_hz(n1);
        let compressor_hz = self.spec.compressor_blade_passage_hz(n1);

        self.fan_phase = advance(self.fan_phase, fan_hz);
        self.shaft_phase = advance(self.shaft_phase, shaft_hz);
        self.compressor_phase = advance(self.compressor_phase, compressor_hz);

        // --- ファンの翼通過音。**これが「キーン」。** ---
        //
        // 倍音はナイキストを超えないものだけ足す。**超えたぶんは折り返して
        // 低い側に化け、金属的な濁りになる。**
        let nyquist = SAMPLE_RATE / 2.0;
        let mut fan = (TAU * self.fan_phase).sin();
        for harmonic in 2..=3 {
            let k = f64::from(harmonic);
            if fan_hz * k < nyquist * 0.95 {
                fan += (TAU * k * self.fan_phase).sin() * (0.35 / k);
            }
        }
        // 回転が上がるほど鋭くなる。
        let spool = ((n1 - self.spec.idle_n1) / (self.spec.max_n1 - self.spec.idle_n1).max(1.0))
            .clamp(0.0, 1.2);
        // 「キーン」の主役だが、**出しすぎると笛になる。**
        // 実測で 4〜9 kHz が全エネルギーの 98% を占めたときは、
        // ジェットではなく単なる高い笛の音だった。排気の広帯域と
        // 釣り合わせること。
        // アイドルの底を低くする。**高いままだと、アイドルが
        // 純粋な笛になる**（実測で 4 kHz 以上が 97%）。
        // 副次的に、出力を上げたときの変化も大きくなる。
        let fan = fan * spool.mul_add(0.95, 0.18);

        // --- バズソー ---
        //
        // 翼端が超音速になったときだけ。**軸回転周波数の倍音**に並ぶので、
        // 翼ごとのばらつきを足し合わせて作る。
        let tip_mach = self.spec.tip_mach(n1);
        let buzz_level = ((tip_mach - 1.0) / 0.45).clamp(0.0, 1.0);
        let buzz = if buzz_level > 0.0 {
            let count = self.blade_gains.len() as f64;
            let mut sum = 0.0;
            for (index, gain) in self.blade_gains.iter().enumerate() {
                // 各翼が軸位相の中の自分の位置を通るときに立つ、鋭いパルス。
                // **同時に鳴っているのは 1 枚だけ**なので、翼数で割らない。
                let phase = fract(self.shaft_phase - index as f64 / count);
                sum += gain * shock_pulse(phase * count);
            }
            sum * buzz_level * 0.45
        } else {
            0.0
        };

        // --- 圧縮機の翼通過音 ---
        let compressor = if compressor_hz < nyquist * 0.95 {
            (TAU * self.compressor_phase).sin() * spool * 0.12
        } else {
            0.0
        };

        // --- 排気の混合音 ---
        //
        // ピークは Strouhal 数 0.2。強度は速度の 6 乗。
        let exhaust_speed = self.spec.exhaust_speed(throttle);
        self.jet_filter
            .set_frequency(self.spec.jet_peak_hz(throttle), 1.1);
        let speed_ratio = exhaust_speed / self.spec.military_exhaust_speed.max(1.0);
        // **強度が 6 乗なら、振幅は 3 乗。** 強度をそのまま振幅に使うと、
        // アフターバーナーで 10 倍以上に跳ねて割れる。
        // **点火したら排気が主役になる。** ファンの翼通過音と同じかそれ以上
        // 出さないと、アフターバーナーが「少し大きくなっただけ」に聞こえる。
        let jet_level = speed_ratio.powi(3) * 0.55;
        let raw = self.jet_noise.tick();
        let jet = (self.jet_filter.tick(raw) * 1.6 + self.jet_body.tick(raw) * 0.55) * jet_level;

        // --- アフターバーナー ---
        let burner_fraction = self.spec.afterburner_fraction(throttle);
        // **1 次ローパスは通す電力を大きく削る**（220 Hz / 24 kHz なので
        // 実効値は入力の 1 割ほど）。点火が分かる大きさにするには、
        // その削られたぶんを見込んだ利得が要る。
        let burner = if burner_fraction > 0.0 {
            self.burner_filter.tick(self.burner_noise.tick()) * burner_fraction * 9.0
        } else {
            // 通していないときも状態は進める。**点火の瞬間に飛ばないため。**
            self.burner_filter.tick(0.0);
            0.0
        };

        // --- 吸気 ---
        let intake = self.intake_filter.tick(self.intake_noise.tick()) * spool * 0.22;

        // 全体を 0.45 倍して出す。**ここで頭を抑えておかないと、
        // ミキサーで soft clip に深く当たり、せっかくの澄んだ翼通過音が
        // 潰れて濁る**（実測でピーク 2.4 まで出ていた）。
        let mixed = (fan.mul_add(0.45, buzz) + compressor + jet + burner + intake) * 0.45;
        self.dc_blocker.tick(mixed)
    }
}

/// 翼端衝撃波 1 発の形（N 波）。
///
/// `position` は 0 が衝撃波の瞬間で、1 で隣の翼の番になる。
///
/// 衝撃波は**正へ跳ね上がってから負へ抜け、0 に戻る**（N 波）。
/// その形をそのまま書く。
///
/// # 2 つの落とし穴
///
/// **1 を超えたら 0 を返すこと。** ここで周期関数（`rem_euclid`）にすると、
/// 1 枚の翼が 1 回転あたり翼数ぶん鳴ってしまい、軸回転ではなく翼通過の
/// 周波数になる。バズソーの定義そのものが崩れる。
///
/// **平均を 0 の近くに置くこと。** 形が片側に寄っていると、翼数ぶん
/// 足し合わせたときに巨大な直流成分になる（一度そう書いて、出力が 4.5 まで
/// 振り切れた）。
fn shock_pulse(position: f64) -> f64 {
    if !(0.0..1.0).contains(&position) {
        return 0.0;
    }
    // 立ち上がりは瞬時、そこから直線的に負へ抜けながら減衰する。
    position.mul_add(-2.0, 1.0) * (-6.0 * position).exp()
}

/// 位相を 1 標本ぶん進める。
fn advance(phase: f64, frequency_hz: f64) -> f64 {
    if !frequency_hz.is_finite() || frequency_hz <= 0.0 {
        return phase;
    }
    fract(phase + frequency_hz / SAMPLE_RATE)
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
// 「点火していない = ちょうど 0」は契約そのもの。**近似で見ると、
// 点いていないはずのアフターバーナーが微かに鳴っていても通ってしまう。**
#[expect(clippy::float_cmp, reason = "点火していない状態は厳密に 0 が契約")]
mod tests {
    use super::*;
    use crate::dsp::tests_support::magnitude_at;

    fn spec() -> TurbineSpec {
        TurbineSpec::default()
    }

    fn render(throttle: f64, seconds: f64) -> Vec<f64> {
        let mut voice = TurbineVoice::new(spec());
        voice.reset(estimate_n1(spec(), throttle), throttle);
        for _ in 0..(SAMPLE_RATE as usize / 2) {
            voice.tick(estimate_n1(spec(), throttle), throttle);
        }
        (0..(SAMPLE_RATE * seconds) as usize)
            .map(|_| voice.tick(estimate_n1(spec(), throttle), throttle))
            .collect()
    }

    fn level(samples: &[f64]) -> f64 {
        (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
    }

    // --- 周波数の根拠 ---

    #[test]
    fn the_fan_tone_is_the_blade_count_times_shaft_speed() {
        // BPF = 羽根数 × rpm/60。32 枚・13,270 rpm で約 7.1 kHz。
        // **これが「キーン」の高さ。**
        let hz = spec().fan_blade_passage_hz(13_270.0);
        assert!((hz - 7_077.0).abs() < 1.0, "got {hz}");
    }

    #[test]
    fn the_fan_tone_is_far_above_a_piston_engine() {
        // ピストンの点火周波数は最大でも 90 Hz。**2 桁違う。**
        let turbine = spec().fan_blade_passage_hz(spec().max_n1);
        assert!(turbine > 5_000.0, "got {turbine}");
    }

    #[test]
    fn the_high_pressure_spool_turns_faster_than_the_fan() {
        // F404 は 16,810 / 13,270。圧縮機の音はファンより高い。
        assert!(spec().spool_ratio > 1.2);
        assert!(
            spec().compressor_blade_passage_hz(10_000.0) > spec().fan_blade_passage_hz(10_000.0)
        );
    }

    #[test]
    fn the_fan_tips_go_supersonic_before_full_power() {
        // **バズソーが出る条件。** 直径 0.89 m・13,270 rpm で翼端は
        // 約 618 m/s、マッハ 1.8。
        assert!(spec().tip_mach(spec().max_n1) > 1.5);
        // アイドルでも超えている（軍用エンジンの特徴）。
        assert!(spec().tip_mach(spec().idle_n1) > 1.0);
    }

    #[test]
    fn the_jet_peak_follows_the_strouhal_relation() {
        // f = 0.2 × u / d。600 m/s・0.6 m で 200 Hz。
        let peak: f64 = 0.2 * 600.0 / 0.6;
        assert!((peak - 200.0).abs() < 1e-9);
        // 出力を上げるとピークが上がること。
        assert!(spec().jet_peak_hz(1.0) > spec().jet_peak_hz(0.2));
    }

    #[test]
    fn the_afterburner_only_lights_at_the_top_of_the_throttle() {
        assert_eq!(spec().afterburner_fraction(0.5), 0.0);
        assert_eq!(
            spec().afterburner_fraction(spec().afterburner_threshold),
            0.0
        );
        assert!(spec().afterburner_fraction(0.95) > 0.0);
        assert!((spec().afterburner_fraction(1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_afterburner_raises_the_exhaust_speed() {
        let military = spec().exhaust_speed(spec().afterburner_threshold);
        let full = spec().exhaust_speed(1.0);
        assert!(full > military * 1.2, "{full} against {military}");
    }

    // --- 実際に鳴らして測る ---

    #[test]
    fn the_sound_has_energy_at_the_fan_blade_passage_frequency() {
        // **「それらしい音がする」ではなく、狙った周波数に山があること。**
        let samples = render(1.0, 1.0);
        let n1 = estimate_n1(spec(), 1.0);
        let bpf = spec().fan_blade_passage_hz(n1);
        let on_tone = magnitude_at(&samples, bpf);
        let beside = magnitude_at(&samples, bpf * 1.21);
        assert!(
            on_tone > beside * 3.0,
            "the fan tone should stand out: {on_tone} at {bpf} Hz vs {beside}"
        );
    }

    #[test]
    fn the_fan_tone_climbs_with_the_throttle() {
        // 出力を上げたらキーンの高さが上がること。**逆だと音が嘘をつく。**
        let low_n1 = estimate_n1(spec(), 0.2);
        let high_n1 = estimate_n1(spec(), 1.0);
        assert!(high_n1 > low_n1);

        let low = render(0.2, 1.0);
        let high = render(1.0, 1.0);
        let low_bpf = spec().fan_blade_passage_hz(low_n1);
        let high_bpf = spec().fan_blade_passage_hz(high_n1);
        assert!(magnitude_at(&low, low_bpf) > magnitude_at(&high, low_bpf));
        assert!(magnitude_at(&high, high_bpf) > magnitude_at(&low, high_bpf));
    }

    #[test]
    fn the_buzz_saw_puts_energy_on_shaft_harmonics_not_just_the_blade_tone() {
        // **翼通過音だけだと電子的な単音になる。** 軸回転の倍音が
        // 並んでいることが、あの濁った鋸の音になる。
        let samples = render(1.0, 1.0);
        let shaft = spec().shaft_hz(estimate_n1(spec(), 1.0));

        // 軸回転の倍音のうち、翼通過音とその倍音に当たらないものを見る。
        let audible = (3..=12)
            .filter(|k| !matches!(k % i32::try_from(spec().fan_blades).unwrap_or(32), 0))
            .filter(|k| magnitude_at(&samples, shaft * f64::from(*k)) > 1e-5)
            .count();
        assert!(
            audible >= 6,
            "expected shaft harmonics from the buzz saw, only {audible} were audible"
        );
    }

    #[test]
    fn there_is_broadband_roar_below_the_tones() {
        // 排気の混合音。**これが無いと「笛」で、ジェットにならない。**
        let samples = render(1.0, 1.0);
        let roar = magnitude_at(&samples, spec().jet_peak_hz(1.0));
        assert!(roar > 1e-4, "the jet roar is inaudible at {roar}");
    }

    #[test]
    fn the_afterburner_makes_it_much_louder() {
        // 点火したら一段大きくなること。**気付かないなら意味がない。**
        let military = level(&render(spec().afterburner_threshold, 0.5));
        let reheat = level(&render(1.0, 0.5));
        assert!(
            reheat > military * 1.3,
            "reheat {reheat} should be well above military {military}"
        );
    }

    #[test]
    fn the_spool_takes_seconds_to_come_up() {
        // **タービンは回転が上がるのが遅い。** ここを速くすると
        // 別の乗り物の音になる。
        let mut voice = TurbineVoice::new(spec());
        voice.reset(spec().idle_n1, 0.0);
        let target = estimate_n1(spec(), 1.0);

        for _ in 0..(SAMPLE_RATE as usize / 2) {
            voice.tick(target, 1.0);
        }
        let after_half_second = voice.n1();
        assert!(
            after_half_second < spec().idle_n1 + (target - spec().idle_n1) * 0.35,
            "the spool came up too fast: {after_half_second} of {target}"
        );

        for _ in 0..(SAMPLE_RATE as usize * 8) {
            voice.tick(target, 1.0);
        }
        assert!(
            (voice.n1() - target).abs() < target * 0.02,
            "the spool never arrived, stuck at {}",
            voice.n1()
        );
    }

    #[test]
    fn the_spool_comes_down_faster_than_it_goes_up() {
        // 実機も落ちる方が速い。
        let target = estimate_n1(spec(), 1.0);
        let idle = spec().idle_n1;

        let mut rising = TurbineVoice::new(spec());
        rising.reset(idle, 0.0);
        for _ in 0..SAMPLE_RATE as usize {
            rising.tick(target, 1.0);
        }
        let climbed = (rising.n1() - idle) / (target - idle);

        let mut falling = TurbineVoice::new(spec());
        falling.reset(target, 1.0);
        for _ in 0..SAMPLE_RATE as usize {
            falling.tick(idle, 0.0);
        }
        let dropped = (target - falling.n1()) / (target - idle);

        assert!(
            dropped > climbed,
            "spool down {dropped} should outpace spool up {climbed}"
        );
    }

    #[test]
    fn the_output_stays_within_range_across_the_whole_envelope() {
        for throttle in [0.0, 0.25, 0.5, 0.9, 0.95, 1.0] {
            let samples = render(throttle, 0.3);
            let peak = samples.iter().fold(0.0_f64, |peak, s| peak.max(s.abs()));
            assert!(peak.is_finite() && peak < 2.2, "peak {peak} at {throttle}");
        }
    }

    #[test]
    fn the_engine_is_audible_at_idle() {
        // タービンはアイドルでも高速で回っていて、はっきり鳴っている。
        let samples = render(0.0, 0.5);
        assert!(level(&samples) > 0.01, "idle is inaudible");
    }

    #[test]
    fn there_is_no_dc_offset() {
        let samples = render(0.8, 1.0);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean.abs() < 0.01, "dc offset {mean}");
    }

    #[test]
    fn the_output_is_the_same_every_time() {
        assert_eq!(render(0.7, 0.2), render(0.7, 0.2));
    }

    #[test]
    fn broken_inputs_do_not_reach_the_speakers() {
        let mut voice = TurbineVoice::new(spec());
        for _ in 0..2_000 {
            let sample = voice.tick(f64::NAN, f64::NAN);
            assert!(sample.is_finite(), "got {sample}");
        }
        for _ in 0..2_000 {
            let sample = voice.tick(f64::INFINITY, 5.0);
            assert!(sample.is_finite(), "got {sample}");
        }
        // 壊れた値のあとでも鳴り直せること。
        let target = estimate_n1(spec(), 0.8);
        let mut peak = 0.0_f64;
        for _ in 0..(SAMPLE_RATE as usize * 4) {
            peak = peak.max(voice.tick(target, 0.8).abs());
        }
        assert!(peak > 0.01, "the voice went silent for good, peak {peak}");
    }

    #[test]
    fn a_broken_throttle_gives_a_sane_n1() {
        for throttle in [f64::NAN, f64::INFINITY, -5.0, 5.0] {
            let n1 = estimate_n1(spec(), throttle);
            assert!(n1.is_finite(), "got {n1}");
            assert!((spec().idle_n1..=spec().max_n1).contains(&n1), "got {n1}");
        }
    }

    #[test]
    fn the_fan_tone_does_not_alias_at_high_speed() {
        // **ナイキストを超えた倍音は折り返して低い側に化ける。**
        // 折り返すと、回転を上げたのに低い音が増えるという逆の挙動になる。
        let samples = render(1.0, 1.0);
        let bpf = spec().fan_blade_passage_hz(estimate_n1(spec(), 1.0));
        // 第 3 倍音は 21 kHz でナイキスト際。折り返し先（48k - 3*bpf）に
        // 山が立っていないこと。
        let mirrored = SAMPLE_RATE - bpf * 3.0;
        if mirrored > 100.0 && mirrored < SAMPLE_RATE / 2.0 {
            let ghost = magnitude_at(&samples, mirrored);
            let real = magnitude_at(&samples, bpf);
            assert!(
                ghost < real * 0.2,
                "an aliased image at {mirrored} Hz reached {ghost} against {real}"
            );
        }
    }
}
