//! ピストンエンジンとプロペラの音。
//!
//! # なぜ正弦波の重ね合わせでは駄目なのか
//!
//! **エンジンの音は、持続する倍音ではなく、排気の圧力パルスの連なり**である
//! （[PTR 論文]）。正弦波を足して作ると、周波数構成は合っていても
//! 「オルガン」や「電子音」にしかならない。実際そうなった。
//!
//! ここではパルスを作り、それを共鳴管に通す（Pulse-Train-Resonator）。
//!
//! # 何を鳴らしているか
//!
//! 対象は軽単発機の代表的な構成（Lycoming O-320 相当）:
//! **水平対向 4 気筒・4 サイクル・直結駆動・固定ピッチ 2 枚羽根**。
//!
//! | 音源 | 周波数 | 由来 |
//! |---|---|---|
//! | 排気 | `rpm × 気筒数 / 2 / 60` | 4 サイクルは 2 回転で全気筒が 1 回ずつ燃える |
//! | プロペラ | `rpm × 羽根数 / 60` | 羽根が観測点を通過する周期（BPF） |
//! | 機械音 | 回転周波数 `rpm / 60` の倍音 | 弁機構・補機 |
//! | 吸気・冷却 | 広帯域雑音 | 回転数と出力で強くなる |
//!
//! **4 気筒 4 サイクル + 2 枚羽根の直結駆動では、排気とプロペラの基本周波数が
//! 一致する**（どちらも `rpm / 30`）。2400 rpm なら 80 Hz。この重なりが、
//! 軽単発機のあの「唸り」の正体である。
//!
//! 実機の音は概ね 80 Hz〜1 kHz の帯域に倍音列として出る。
//!
//! # 何を写していないか
//!
//! - **排気管の形状も長さも実機から取っていない。** 共鳴周波数は
//!   「それらしく響く」値を置いている
//! - 気筒ごとの点火間隔のばらつき、失火、混合比の影響
//! - プロペラ後流と機体の干渉音、翼面での反射
//! - ドップラー、聴取位置による違い（コックピット内外で同じ音）
//!
//! [PTR 論文]: https://arxiv.org/html/2603.09391v1

use std::f64::consts::TAU;

use crate::dsp::{DelayResonator, HighPass, LowPass, Noise, Resonator, SAMPLE_RATE, Smoothed};

/// エンジンとプロペラの諸元。
///
/// **音のためだけの諸元。** FDM の `AircraftConfig` は回転数を持たない
/// （出力で推力を出すモデルなので）。ここは音を作るのに要る値を別に持つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineSpec {
    /// 気筒数。
    pub cylinders: u32,
    /// 1 サイクルの行程数。4 サイクルなら 4。
    pub strokes: u32,
    /// プロペラの羽根数。
    pub blades: u32,
    /// 減速比（エンジン回転 : プロペラ回転）。直結駆動なら 1.0。
    pub propeller_gear_ratio: f64,
    /// アイドル回転数 `rpm`。
    pub idle_rpm: f64,
    /// 最大回転数 `rpm`。
    pub max_rpm: f64,
}

impl Default for EngineSpec {
    /// Lycoming O-320 相当。
    ///
    /// 水平対向 4 気筒・4 サイクル・空冷・**直結駆動**、150 hp / 2700 rpm。
    /// 暖機は 1000〜1200 rpm、巡航 2350〜2450 rpm。
    /// 固定ピッチプロペラは 2 枚羽根とする。
    ///
    /// アイドルは公表値ではなく、暖機回転数より下の慣用値（軽単発機の
    /// 地上アイドルはおおむね 600〜800 rpm）として 700 を置く。
    fn default() -> Self {
        Self {
            cylinders: 4,
            strokes: 4,
            blades: 2,
            propeller_gear_ratio: 1.0,
            idle_rpm: 700.0,
            max_rpm: 2_700.0,
        }
    }
}

impl EngineSpec {
    /// 排気の点火周波数 `Hz`。
    ///
    /// 4 サイクルは 2 回転で全気筒が 1 回ずつ燃えるので、
    /// `rpm/60 × 気筒数 / (行程数/2)`。4 気筒 4 サイクルなら `rpm/30`。
    #[must_use]
    pub fn firing_hz(self, rpm: f64) -> f64 {
        let revolutions_per_second = rpm / 60.0;
        let cycles_per_revolution = 2.0 / f64::from(self.strokes.max(1));
        revolutions_per_second * f64::from(self.cylinders) * cycles_per_revolution
    }

    /// プロペラの羽根通過周波数 `Hz`（BPF = 羽根数 × 回転数 / 60）。
    #[must_use]
    pub fn blade_passage_hz(self, rpm: f64) -> f64 {
        let propeller_rpm = rpm / self.propeller_gear_ratio.max(0.01);
        propeller_rpm / 60.0 * f64::from(self.blades)
    }

    /// 回転周波数 `Hz`。機械音の基準。
    #[must_use]
    pub fn shaft_hz(self, rpm: f64) -> f64 {
        rpm / 60.0
    }
}

/// スロットルと対気速度から回転数を見積もる。
///
/// # これは推定であって、物理ではない
///
/// **FDM は回転数を持っていない。** `EngineConfig` は軸出力とプロペラ効率
/// から推力を出すモデルで、回転数という状態が無い。音のためだけに、
/// 固定ピッチプロペラの振る舞いを模した式をここに置く。
///
/// 固定ピッチでは、回転数はスロットルだけでは決まらない。速度が上がると
/// プロペラの迎角が下がって負荷が抜け、同じスロットルでも回転が上がる
/// （地上で全開にしても定格回転に届かず、上昇していくと回転が上がるのは
/// これによる）。その効果を線形で入れてある。
///
/// **飛び方には一切影響しない。** 音以外からは参照しないこと。
#[must_use]
pub fn estimate_rpm(spec: EngineSpec, throttle: f64, airspeed_ms: f64) -> f64 {
    let throttle = if throttle.is_finite() {
        throttle.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let airspeed = if airspeed_ms.is_finite() {
        airspeed_ms.clamp(0.0, 150.0)
    } else {
        0.0
    };

    // スロットルで決まるぶん。全閉でアイドル、全開で最大回転の 88%。
    // **全開でいきなり定格に張り付かせない。** 残りを速度に取っておく。
    let span = (spec.max_rpm - spec.idle_rpm).max(0.0);
    let from_throttle = throttle.mul_add(span * 0.88, spec.idle_rpm);

    // 速度で負荷が抜けるぶん。60 m/s（約 117 kt）で最大回転の 12% を足す。
    // 出力を出していないときはプロペラが空回りするだけなので、
    // スロットルに比例させる。
    let unloading = (airspeed / 60.0).min(1.2) * span * 0.12 * throttle;

    (from_throttle + unloading).clamp(0.0, spec.max_rpm * 1.05)
}

/// 気筒 1 本ぶんの排気パルス。
///
/// 位相が一周するたびに 1 回燃える。**位相を進めるのはこの構造体の外**
/// （全気筒が同じエンジン位相を共有するため）。
#[derive(Debug, Clone, Copy)]
struct Cylinder {
    /// エンジン位相に対するこの気筒の点火位置 `[0, 1)`。
    offset: f64,
    /// この気筒の音量。**完全に揃えない**（実機は気筒ごとに少し違う）。
    gain: f64,
}

/// エンジン音を 1 標本ずつ作る。
#[derive(Debug)]
pub struct EngineVoice {
    spec: EngineSpec,
    cylinders: Vec<Cylinder>,
    /// 気筒数。`u32` から作るので、`f64` への変換で精度は落ちない。
    /// 毎標本使うので一度だけ変換して持つ。
    cylinder_count: f64,

    /// **エンジン 1 サイクル**を 0→1 で回る位相。
    ///
    /// 点火の周期ではなくサイクルの周期であることが要点。4 サイクルの
    /// 1 サイクルは 2 回転で、その間に全気筒が 1 回ずつ燃える。
    /// 気筒をこの位相の中へ等間隔に置くと、パルスは点火周波数で出る。
    ///
    /// **点火周波数を基準に気筒を並べると、パルスが気筒数ぶん速くなる**
    /// （一度そう書いて、2400 rpm で 80 Hz のはずが 320 Hz になっていた）。
    cycle_phase: f64,
    /// プロペラの羽根通過位相。
    blade_phase: f64,
    /// 軸回転の位相。機械音に使う。
    shaft_phase: f64,

    /// 排気管の共鳴。
    exhaust_pipe: DelayResonator,
    /// 排気管の共鳴（フォルマント）。
    ///
    /// **回転数では動かない。** 管の長さで決まるので、回転を上げても
    /// ここは同じ周波数で鳴る。動く倍音列と動かない共鳴の組み合わせが、
    /// 「物体が鳴っている」という印象を作る。ここを回転数に追従させると、
    /// 全体が移調するだけの電子音に戻る。
    formants: [Resonator; 3],
    /// 機体（キャビン）の共鳴。低い方を持ち上げる。
    body: Resonator,
    /// 吸気・冷却の広帯域雑音。
    intake_noise: Noise,
    intake_filter: Resonator,
    /// プロペラ後流の広帯域雑音。
    slipstream_noise: Noise,
    slipstream_filter: LowPass,
    /// 直流を切る。**直流が残るとスピーカーを押しっぱなしにする。**
    dc_blocker: HighPass,

    /// なめらかに追わせる制御値。
    rpm: Smoothed,
    load: Smoothed,
}

impl EngineVoice {
    /// 諸元を決めて作る。
    #[must_use]
    pub fn new(spec: EngineSpec) -> Self {
        // 気筒の点火位置。**等間隔に置く。** 実機の水平対向 4 気筒は
        // 点火順序こそあるが、点火間隔は等しい（720° / 4 = 180°）。
        let count = spec.cylinders.max(1);
        let cylinders = (0..count)
            .map(|index| Cylinder {
                offset: f64::from(index) / f64::from(count),
                // 気筒ごとにわずかに音量を変える。**完全に揃えると
                // 機械的に整いすぎて、かえって嘘っぽくなる。**
                gain: 1.0 + (f64::from(index) * 0.7).sin() * 0.06,
            })
            .collect();

        Self {
            spec,
            cylinders,
            cylinder_count: f64::from(count),
            cycle_phase: 0.0,
            blade_phase: 0.0,
            shaft_phase: 0.0,
            // 排気管の共鳴。**実機の管長から出した値ではない。**
            // 130 Hz は 4 気筒機の排気音として据わりのよい低さ。
            exhaust_pipe: DelayResonator::new(130.0, 0.55, 3_500.0),
            // 排気管のフォルマント。**実機の管長から出した値ではない。**
            // 軽単発機の音が乗っている 300 Hz〜1.2 kHz に 3 本置いてある。
            formants: [
                Resonator::new(320.0, 6.0),
                Resonator::new(650.0, 5.0),
                Resonator::new(1_150.0, 4.0),
            ],
            // キャビンの共鳴。低音に体積感を出す。
            body: Resonator::new(190.0, 3.0),
            intake_noise: Noise::new(0x51a1_2b3c_4d5e_6f70),
            intake_filter: Resonator::new(900.0, 1.2),
            slipstream_noise: Noise::new(0x9e37_79b9_7f4a_7c15),
            slipstream_filter: LowPass::new(1_800.0),
            dc_blocker: HighPass::new(45.0),
            rpm: Smoothed::new(spec.idle_rpm, 0.12),
            // 出力の追従はゆっくり。**急に変えると「ブツッ」と鳴る。**
            load: Smoothed::new(0.0, 0.15),
        }
    }

    /// 諸元。
    #[must_use]
    pub const fn spec(&self) -> EngineSpec {
        self.spec
    }

    /// 今なぞっている回転数。
    #[must_use]
    pub const fn rpm(&self) -> f64 {
        self.rpm.value()
    }

    /// 開始時の値へ飛ばす。やり直しの瞬間に使う（渡り音を出さないため）。
    pub const fn reset(&mut self, rpm: f64, load: f64) {
        self.rpm.reset(rpm);
        self.load.reset(load);
    }

    /// 1 標本作る。`target_rpm` と `load`（出力 `[0,1]`）へ追従する。
    pub fn tick(&mut self, target_rpm: f64, load: f64) -> f64 {
        let rpm = self.rpm.tick(target_rpm).max(0.0);
        let load = self.load.tick(load).clamp(0.0, 1.0);

        let firing_hz = self.spec.firing_hz(rpm);
        let blade_hz = self.spec.blade_passage_hz(rpm);
        let shaft_hz = self.spec.shaft_hz(rpm);
        // 1 サイクルの間に全気筒が 1 回ずつ燃えるので、サイクルの周波数は
        // 点火周波数を気筒数で割ったもの。
        let cycle_hz = firing_hz / self.cylinder_count;

        self.cycle_phase = advance(self.cycle_phase, cycle_hz);
        self.blade_phase = advance(self.blade_phase, blade_hz);
        self.shaft_phase = advance(self.shaft_phase, shaft_hz);

        // --- 排気パルス ---
        //
        // 気筒ごとに、点火からの経過を 0→1 で見て、圧力の立ち上がりと
        // 抜けを掛ける。**正弦波ではなくパルスなので、倍音が自然に出る。**
        let mut exhaust = 0.0;
        for cylinder in &self.cylinders {
            let phase = fract(self.cycle_phase - cylinder.offset);
            // **点火間隔で正規化する。** 生のサイクル位相を渡すと、
            // 気筒数が増えるほど 1 発が長く尾を引き、隣の点火と重なって
            // 潰れる。点火間隔の中での経過として渡す。
            let within_interval = phase * self.cylinder_count;
            exhaust += cylinder.gain * exhaust_pulse(within_interval.min(1.0), load);
        }
        exhaust /= self.cylinder_count.sqrt();

        // 排気管に通す。ここで「管を通った音」になる。
        let piped = self.exhaust_pipe.tick(exhaust * 0.6);
        // フォルマントで中域を立てる。**ここが無いと低音の唸りだけになり、
        // 「遠くで何かが低く鳴っている」音にしかならない**（実測で
        // 全エネルギーの 74% が 100 Hz 以下に偏っていた）。
        // 出力が上がるほど強く出す（筒内圧が高いほど励振が強い）。
        let formant_gain = load.mul_add(0.5, 0.5);
        let formants: f64 = self
            .formants
            .iter_mut()
            .zip([0.55, 0.40, 0.25])
            .map(|(resonator, weight)| resonator.tick(exhaust) * weight)
            .sum();
        // 機体の共鳴を少し足して体積感を出す。
        let with_body = piped + self.body.tick(piped) * 0.35 + formants * formant_gain;

        // --- プロペラ ---
        //
        // 羽根の通過は排気より丸い（圧力場の変化で、爆発ではない）。
        // 回転数が上がると羽根先が速くなり、音が急に大きくなる。
        let tip_factor = (rpm / self.spec.max_rpm.max(1.0)).clamp(0.0, 1.3);

        // 純音では「サイレン」になる。**実機のプロペラ音は、羽根の通過で
        // 強弱の付いた空気の音**（周期的な荷重音と広帯域の渦音の重ね合わせ）。
        // 基音を控えめに置き、同じ周期で雑音に強弱を付ける。
        let tone =
            (TAU * self.blade_phase).sin() * 0.6 + (TAU * 2.0 * self.blade_phase).sin() * 0.2;
        // 羽根が通る瞬間だけ持ち上がる窓。負にならないよう 0 で切る。
        let gust = (TAU * self.blade_phase).cos().mul_add(0.5, 0.5).powi(3);
        let propeller = (tone * 0.5 + gust * 0.9) * tip_factor * tip_factor * 0.40;

        // --- 機械音 ---
        //
        // 弁機構は 2 回転に 1 回動く。回転周波数の半分の倍音として出る。
        let mechanical = (TAU * self.shaft_phase * 0.5).sin() * 0.05
            + (TAU * self.shaft_phase * 1.5).sin() * 0.04;

        // --- 広帯域 ---
        //
        // 吸気は出力に、後流は回転数に付いてくる。
        let intake = self.intake_filter.tick(self.intake_noise.tick()) * load * 0.30;
        self.slipstream_filter
            .set_cutoff(600.0 + tip_factor * 2_400.0);
        // 後流の雑音も羽根の通過で揺らす。**一定の「シャー」だと
        // 扇風機にもエンジンにも聞こえない。**
        let slipstream = self.slipstream_filter.tick(self.slipstream_noise.tick())
            * tip_factor
            * gust.mul_add(0.6, 0.4)
            * 0.34;

        // アイドルでも鳴っている。**全閉で無音になると、
        // エンストしたのか音が壊れたのか分からない。**
        let level = load.mul_add(0.55, 0.45);
        let mixed = (with_body + propeller + mechanical + intake + slipstream) * level;
        self.dc_blocker.tick(mixed)
    }
}

/// 排気パルス 1 発の形。
///
/// `phase` は点火からの経過 `[0, 1)`。
///
/// # 形の作り方
///
/// [PTR 論文] の圧力開放エンベロープ `(1 - e^{-αφ}) · e^{-βφ}` を採る。
/// 立ち上がりと減衰を別々に決められるので、**急に開いてゆっくり抜ける**
/// という排気の非対称な形をそのまま書ける。
///
/// 出力を上げると立ち上がりが鋭くなる（筒内圧が高いほど開放が急になる）。
/// これが「スロットルを開けると音が硬くなる」の正体で、**音量と高さだけを
/// 変えても再現できない**部分である。
///
/// [PTR 論文]: https://arxiv.org/html/2603.09391v1
fn exhaust_pulse(phase: f64, load: f64) -> f64 {
    // 立ち上がりの鋭さ。出力が高いほど急。
    let attack = load.mul_add(90.0, 45.0);
    // 抜けの速さ。**大きいほど短いパルスになり、高い倍音が増える。**
    // 小さくすると点火間隔いっぱいに広がって、低音の唸りだけになる。
    // 出力が上がるほど短く鋭くする。
    let decay = load.mul_add(9.0, 11.0);
    let envelope = (-attack * phase).exp().mul_add(-1.0, 1.0) * (-decay * phase).exp();

    // 位相をゆがめる。**燃焼直後の高温ガスは音速が高く、抜けるにつれて
    // 下がる。** その結果、パルスの中で音の高さが下がっていく。
    // 論文の「熱力学的位相変調」を、指数 0.75 の固定で入れる。
    let warped = phase.powf(0.75);

    // ゆがめた位相に倍音を乗せる。上の倍音ほど弱くする
    // （高域が空気と管で早く減衰するため）。
    // 減衰は出力で変える。出力が上がると高い倍音まで残る＝音が硬くなる。
    let rolloff = load.mul_add(-0.16, 0.34);
    let mut value = 0.0;
    for harmonic in 1..=10 {
        let k = f64::from(harmonic);
        value += (TAU * k * warped).sin() * (-rolloff * k).exp();
    }
    envelope * value
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
mod tests {
    use super::*;
    use crate::dsp::tests_support::magnitude_at;

    fn spec() -> EngineSpec {
        EngineSpec::default()
    }

    /// 定常回転で `seconds` 秒ぶん鳴らす。
    fn render(rpm: f64, load: f64, seconds: f64) -> Vec<f64> {
        let mut voice = EngineVoice::new(spec());
        voice.reset(rpm, load);
        let count = (SAMPLE_RATE * seconds) as usize;
        // 共鳴が落ち着くまで捨てる。**立ち上がりを測ると定常の特性が出ない。**
        for _ in 0..(SAMPLE_RATE as usize / 4) {
            voice.tick(rpm, load);
        }
        (0..count).map(|_| voice.tick(rpm, load)).collect()
    }

    // --- 周波数の根拠 ---

    #[test]
    fn the_firing_frequency_matches_the_four_stroke_formula() {
        // 4 サイクル 4 気筒は 2 回転で 4 回燃える = rpm/30。
        // 2400 rpm で 80 Hz。**外部の公式と突き合わせる。**
        assert!((spec().firing_hz(2_400.0) - 80.0).abs() < 1e-9);
        assert!((spec().firing_hz(2_700.0) - 90.0).abs() < 1e-9);
        assert!((spec().firing_hz(700.0) - 700.0 / 30.0).abs() < 1e-9);
    }

    #[test]
    fn the_blade_passage_frequency_matches_the_bpf_formula() {
        // BPF = 羽根数 × rpm / 60。2 枚・2400 rpm で 80 Hz。
        assert!((spec().blade_passage_hz(2_400.0) - 80.0).abs() < 1e-9);
    }

    #[test]
    fn the_exhaust_and_the_propeller_land_on_the_same_note() {
        // **4 気筒 4 サイクル + 2 枚羽根の直結駆動では両者が一致する。**
        // 軽単発機のあの唸りはこの重なり。ここがずれていたら諸元が違う。
        for rpm in [700.0, 1_200.0, 2_400.0, 2_700.0] {
            assert!(
                (spec().firing_hz(rpm) - spec().blade_passage_hz(rpm)).abs() < 1e-9,
                "at {rpm} rpm: {} vs {}",
                spec().firing_hz(rpm),
                spec().blade_passage_hz(rpm)
            );
        }
    }

    #[test]
    fn a_six_cylinder_fires_more_often_than_a_four() {
        let six = EngineSpec {
            cylinders: 6,
            ..spec()
        };
        assert!(six.firing_hz(2_400.0) > spec().firing_hz(2_400.0));
        // 6 気筒 4 サイクルは rpm/20。2400 rpm で 120 Hz。
        assert!((six.firing_hz(2_400.0) - 120.0).abs() < 1e-9);
    }

    #[test]
    fn a_geared_propeller_turns_slower_than_the_engine() {
        let geared = EngineSpec {
            propeller_gear_ratio: 2.0,
            ..spec()
        };
        assert!(geared.blade_passage_hz(2_400.0) < spec().blade_passage_hz(2_400.0));
    }

    // --- 実際に鳴らして測る ---

    #[test]
    fn the_sound_actually_has_energy_at_the_firing_frequency() {
        // **「それらしい音がする」ではなく、狙った周波数に山があること。**
        let samples = render(2_400.0, 0.8, 1.0);
        let firing = magnitude_at(&samples, 80.0);
        let between = magnitude_at(&samples, 123.0);
        assert!(
            firing > between * 2.0,
            "the firing frequency should stand out: {firing} at 80 Hz vs {between} at 123 Hz"
        );
    }

    #[test]
    fn the_note_follows_the_rpm() {
        // 回転を上げたら基音も上がること。**逆だと音が状態を誤って伝える。**
        //
        // 互いの倍音にならない 2 点で比べる。1800 rpm は 60 Hz、
        // 2700 rpm は 90 Hz で、どちらも相手の整数倍ではない。
        // （1200 と 2400 で比べると、40 Hz と 80 Hz が倍音関係にあるうえ、
        // 40 Hz は直流カット（45 Hz）に削られて比較にならない。）
        let low = render(1_800.0, 0.5, 1.0);
        let high = render(2_700.0, 0.5, 1.0);

        assert!(
            magnitude_at(&low, 60.0) > magnitude_at(&high, 60.0),
            "60 Hz should belong to the slower engine"
        );
        assert!(
            magnitude_at(&high, 90.0) > magnitude_at(&low, 90.0),
            "90 Hz should belong to the faster engine"
        );
    }

    #[test]
    fn the_sound_is_rich_in_harmonics_not_a_pure_tone() {
        // **正弦波の重ね合わせから離れたことの検査。**
        // パルス列なので、基音の上に倍音が並ぶ。
        let samples = render(2_400.0, 0.8, 1.0);
        let fundamental = magnitude_at(&samples, 80.0);
        let harmonics: Vec<f64> = (2..=6)
            .map(|k| magnitude_at(&samples, 80.0 * f64::from(k)))
            .collect();
        let audible = harmonics
            .iter()
            .filter(|magnitude| **magnitude > fundamental * 0.05)
            .count();
        assert!(
            audible >= 3,
            "expected several audible harmonics, got {harmonics:?} against {fundamental}"
        );
    }

    #[test]
    fn opening_the_throttle_changes_the_timbre_not_just_the_level() {
        // **音量と高さだけを変えても「スロットルを開けた」感じは出ない。**
        // 出力が上がるとパルスの立ち上がりが鋭くなり、高い倍音が増える。
        // 同じ回転数で比べて、倍音の比が変わることを見る。
        let quiet = render(2_400.0, 0.15, 1.0);
        let loud = render(2_400.0, 1.0, 1.0);

        let ratio = |samples: &[f64]| {
            let fundamental = magnitude_at(samples, 80.0).max(1e-12);
            magnitude_at(samples, 400.0) / fundamental
        };
        assert!(
            ratio(&loud) > ratio(&quiet) * 1.15,
            "the spectrum should get brighter with power: {} vs {}",
            ratio(&loud),
            ratio(&quiet)
        );
    }

    #[test]
    fn the_output_stays_within_range_across_the_whole_envelope() {
        // **どの設定でも割れないこと。** 割れる設定が 1 つでもあると、
        // そこだけ耳障りな音になる。
        for rpm in [0.0, 700.0, 1_500.0, 2_700.0, 3_500.0] {
            for load in [0.0, 0.5, 1.0] {
                let samples = render(rpm, load, 0.3);
                let peak = samples.iter().fold(0.0_f64, |peak, s| peak.max(s.abs()));
                assert!(
                    peak.is_finite() && peak < 1.6,
                    "peak {peak} at {rpm} rpm, load {load}"
                );
            }
        }
    }

    #[test]
    fn the_engine_is_audible_at_idle() {
        // 全閉で無音になると、エンストしたのか音が壊れたのか分からない。
        let samples = render(700.0, 0.0, 0.5);
        let peak = samples.iter().fold(0.0_f64, |peak, s| peak.max(s.abs()));
        assert!(peak > 0.02, "idle is inaudible at {peak}");
    }

    #[test]
    fn there_is_no_dc_offset() {
        // **直流が残るとスピーカーを押しっぱなしにする。**
        // 音としては聞こえないまま、歪みと発熱だけが増える。
        let samples = render(2_400.0, 0.8, 1.0);
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean.abs() < 0.01, "dc offset {mean}");
    }

    #[test]
    fn the_output_is_the_same_every_time() {
        assert_eq!(render(2_400.0, 0.7, 0.2), render(2_400.0, 0.7, 0.2));
    }

    #[test]
    fn broken_inputs_do_not_reach_the_speakers() {
        let mut voice = EngineVoice::new(spec());
        for _ in 0..1_000 {
            let sample = voice.tick(f64::NAN, f64::NAN);
            assert!(sample.is_finite(), "got {sample}");
        }
        for _ in 0..1_000 {
            let sample = voice.tick(f64::INFINITY, 2.0);
            assert!(sample.is_finite(), "got {sample}");
        }
        // 壊れた値のあとでも、まともな値で鳴り直せること。
        let mut peak = 0.0_f64;
        for _ in 0..10_000 {
            peak = peak.max(voice.tick(2_400.0, 0.8).abs());
        }
        assert!(peak > 0.02, "the voice went silent for good, peak {peak}");
    }

    #[test]
    fn the_rpm_moves_smoothly_rather_than_jumping() {
        // **回転数を即座に飛ばすと「ブツッ」と鳴る。**
        let mut voice = EngineVoice::new(spec());
        voice.reset(700.0, 0.0);
        voice.tick(2_700.0, 1.0);
        assert!(
            voice.rpm() < 800.0,
            "the rpm jumped straight to {}",
            voice.rpm()
        );
        for _ in 0..(SAMPLE_RATE as usize) {
            voice.tick(2_700.0, 1.0);
        }
        assert!(
            (voice.rpm() - 2_700.0).abs() < 20.0,
            "the rpm never arrived, stuck at {}",
            voice.rpm()
        );
    }

    // --- 回転数の推定 ---

    #[test]
    fn the_throttle_sets_the_rpm_between_idle_and_maximum() {
        let idle = estimate_rpm(spec(), 0.0, 0.0);
        let full = estimate_rpm(spec(), 1.0, 0.0);
        assert!((idle - spec().idle_rpm).abs() < 1.0);
        assert!(full > idle);
        assert!(full <= spec().max_rpm, "got {full}");
    }

    #[test]
    fn a_fixed_pitch_propeller_unloads_as_the_aircraft_speeds_up() {
        // **地上で全開にしても定格に届かず、速度が乗ると回転が上がる。**
        // 固定ピッチの実機の振る舞い。
        let standing = estimate_rpm(spec(), 1.0, 0.0);
        let cruising = estimate_rpm(spec(), 1.0, 60.0);
        assert!(
            cruising > standing,
            "airspeed should unload the propeller: {cruising} vs {standing}"
        );
        assert!(cruising <= spec().max_rpm * 1.05);
    }

    #[test]
    fn the_propeller_does_not_unload_with_the_throttle_closed() {
        // 出力を出していなければ、速度が乗っても回転は上がらない。
        let slow = estimate_rpm(spec(), 0.0, 0.0);
        let fast = estimate_rpm(spec(), 0.0, 80.0);
        assert!((slow - fast).abs() < 1.0, "{slow} vs {fast}");
    }

    #[test]
    fn a_broken_input_gives_a_sane_rpm() {
        for (throttle, airspeed) in [
            (f64::NAN, 40.0),
            (0.5, f64::NAN),
            (f64::INFINITY, f64::INFINITY),
            (-5.0, -5.0),
        ] {
            let rpm = estimate_rpm(spec(), throttle, airspeed);
            assert!(rpm.is_finite(), "got {rpm}");
            assert!((0.0..=spec().max_rpm * 1.05).contains(&rpm), "got {rpm}");
        }
    }
}
