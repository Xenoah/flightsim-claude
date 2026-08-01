//! 国際標準大気（ISA / U.S. Standard Atmosphere 1976）。
//!
//! 対気速度・揚力・抗力・推力の全てが空気密度に比例するため、大気モデルの誤差は
//! そのまま飛行特性の誤差になる。ここは既知の標準値と照合できる数少ない領域なので、
//! テストで厳密に固定してある。
//!
//! # 実装範囲
//!
//! 幾何高度 -5 km 〜 86 km。実用上の全ての航空機の運用高度を含む。
//!
//! # 非標準日
//!
//! 実際の大気は ISA からずれる。[`Atmosphere::with_temperature_offset`] で
//! 温度偏差（ISA+15 のような値）を与えられる。天候システムはここを経由して
//! FDM に影響する。

use flightsim_core::{Kelvin, KilogramsPerCubicMeter, Meters, MetersPerSecond, Pascals};

/// 標準重力加速度 `m/s²`。ISA の定義値。
pub const STANDARD_GRAVITY: f64 = 9.806_65;

/// 乾燥空気の比気体定数 `J/(kg·K)`。ISA の普遍気体定数 8.314_32 を
/// モル質量 0.028_964_4 kg/mol で割った値。
pub const SPECIFIC_GAS_CONSTANT: f64 = 287.052_874;

/// 空気の比熱比。音速の計算に使う。
pub const HEAT_CAPACITY_RATIO: f64 = 1.4;

/// ISA が幾何高度→ジオポテンシャル高度の換算に用いる実効地球半径 `m`。
///
/// WGS84 の長半径とは異なる値であることに注意。ISA の定義がこの値を使っている。
pub const EFFECTIVE_EARTH_RADIUS: f64 = 6_356_766.0;

/// 海面標準気圧 `Pa`。
pub const SEA_LEVEL_PRESSURE: f64 = 101_325.0;

/// 海面標準温度 `K`（15 °C）。
pub const SEA_LEVEL_TEMPERATURE: f64 = 288.15;

/// 海面標準密度 `kg/m³`。
pub const SEA_LEVEL_DENSITY: f64 =
    SEA_LEVEL_PRESSURE / (SPECIFIC_GAS_CONSTANT * SEA_LEVEL_TEMPERATURE);

/// ISA の 1 層。境界値はジオポテンシャル高度で定義される。
#[derive(Debug, Clone, Copy)]
struct Layer {
    /// 層の下端のジオポテンシャル高度 `m`。
    base_altitude: f64,
    /// 下端の温度 `K`。
    base_temperature: f64,
    /// 下端の気圧 `Pa`。
    base_pressure: f64,
    /// 気温減率 `K/m`。負が「高度とともに冷える」。
    lapse_rate: f64,
}

/// ISA の層構造。境界の気圧は公表値。
/// 隣接層の連続性は `layer_boundaries_are_continuous` で検査している。
const LAYERS: [Layer; 7] = [
    Layer {
        base_altitude: 0.0,
        base_temperature: 288.15,
        base_pressure: 101_325.0,
        lapse_rate: -0.006_5,
    },
    Layer {
        base_altitude: 11_000.0,
        base_temperature: 216.65,
        base_pressure: 22_632.06,
        lapse_rate: 0.0,
    },
    Layer {
        base_altitude: 20_000.0,
        base_temperature: 216.65,
        base_pressure: 5_474.889,
        lapse_rate: 0.001,
    },
    Layer {
        base_altitude: 32_000.0,
        base_temperature: 228.65,
        base_pressure: 868.018_7,
        lapse_rate: 0.002_8,
    },
    Layer {
        base_altitude: 47_000.0,
        base_temperature: 270.65,
        base_pressure: 110.906_3,
        lapse_rate: 0.0,
    },
    Layer {
        base_altitude: 51_000.0,
        base_temperature: 270.65,
        base_pressure: 66.938_87,
        lapse_rate: -0.002_8,
    },
    Layer {
        base_altitude: 71_000.0,
        base_temperature: 214.65,
        base_pressure: 3.956_420,
        lapse_rate: -0.002,
    },
];

/// モデルが扱う幾何高度の下限 `m`。死海（-430 m）にも余裕を持って対応する。
const MIN_GEOMETRIC_ALTITUDE: f64 = -5_000.0;

/// モデルが扱う幾何高度の上限 `m`。ISA の定義域の上端。
const MAX_GEOMETRIC_ALTITUDE: f64 = 86_000.0;

/// ある高度における大気の状態。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtmosphereSample {
    pub temperature: Kelvin,
    pub pressure: Pascals,
    pub density: KilogramsPerCubicMeter,
    pub speed_of_sound: MetersPerSecond,
}

impl AtmosphereSample {
    /// 海面標準密度に対する比。推力モデルなどで使う。
    #[must_use]
    pub fn density_ratio(self) -> f64 {
        self.density.get() / SEA_LEVEL_DENSITY
    }

    /// 与えられた真対気速度に対するマッハ数。
    #[must_use]
    pub fn mach(self, true_airspeed: MetersPerSecond) -> f64 {
        true_airspeed.get() / self.speed_of_sound.get()
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.temperature.is_finite()
            && self.pressure.is_finite()
            && self.density.is_finite()
            && self.speed_of_sound.is_finite()
    }
}

/// 大気モデル。ISA からの温度偏差を保持できる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atmosphere {
    /// ISA 標準温度からの偏差 `K`。`ISA+15` なら `15.0`。
    temperature_offset: f64,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self::standard()
    }
}

impl Atmosphere {
    /// 標準大気（偏差なし）。
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            temperature_offset: 0.0,
        }
    }

    /// ISA からの温度偏差を指定する。
    ///
    /// 気圧は標準のまま、温度と（その帰結として）密度が変化する。
    /// これは航空分野で「ISA+15」等と呼ばれる慣習に対応する扱い。
    /// 実大気の気圧変化（QNH）を再現するものではない。
    #[must_use]
    pub const fn with_temperature_offset(offset: f64) -> Self {
        Self {
            temperature_offset: offset,
        }
    }

    #[must_use]
    pub const fn temperature_offset(self) -> f64 {
        self.temperature_offset
    }

    /// 幾何高度における大気状態を求める。
    ///
    /// 定義域外の高度はクランプされる。**NaN を返すことはない。**
    /// 高高度で発散した機体が NaN を撒き散らすのを防ぐため、意図的にこうしている。
    #[must_use]
    pub fn sample(self, geometric_altitude: Meters) -> AtmosphereSample {
        // `f64::clamp` は NaN 入力に対して NaN を返すため、クランプだけでは守れない。
        // ここで潰しておかないと、NaN が温度・気圧・密度を経由して全状態に伝播する。
        let raw = geometric_altitude.get();
        let z = if raw.is_nan() {
            0.0
        } else {
            raw.clamp(MIN_GEOMETRIC_ALTITUDE, MAX_GEOMETRIC_ALTITUDE)
        };

        // ISA の層はジオポテンシャル高度で定義されている。
        // 幾何高度との差は 11 km で約 19 m、30 km では約 140 m あり、無視できない。
        let h = geopotential_altitude(z);
        let layer = layer_for(h);

        let standard_temperature =
            layer.base_temperature + layer.lapse_rate * (h - layer.base_altitude);
        let pressure = layer_pressure(layer, h, standard_temperature);

        // 温度偏差は温度と密度にのみ効かせる。気圧は標準のまま。
        let temperature = standard_temperature + self.temperature_offset;

        // 温度偏差が極端な場合に絶対零度を割らないよう下限を設ける。
        // ここを守らないと密度が負になり、揚力の符号が反転する。
        let temperature = temperature.max(1.0);

        let density = pressure / (SPECIFIC_GAS_CONSTANT * temperature);
        let speed_of_sound = (HEAT_CAPACITY_RATIO * SPECIFIC_GAS_CONSTANT * temperature).sqrt();

        AtmosphereSample {
            temperature: Kelvin(temperature),
            pressure: Pascals(pressure),
            density: KilogramsPerCubicMeter(density),
            speed_of_sound: MetersPerSecond(speed_of_sound),
        }
    }
}

/// 幾何高度 → ジオポテンシャル高度 `m`。
///
/// 重力が高度とともに弱まる効果を高度側に畳み込む変換。
fn geopotential_altitude(geometric: f64) -> f64 {
    EFFECTIVE_EARTH_RADIUS * geometric / (EFFECTIVE_EARTH_RADIUS + geometric)
}

/// ジオポテンシャル高度が属する層を返す。定義域下端より下は最下層を使う。
fn layer_for(geopotential: f64) -> Layer {
    let mut selected = LAYERS[0];
    for layer in LAYERS {
        if geopotential >= layer.base_altitude {
            selected = layer;
        } else {
            break;
        }
    }
    selected
}

/// 層内の気圧 `Pa`。
fn layer_pressure(layer: Layer, geopotential: f64, temperature: f64) -> f64 {
    if layer.lapse_rate.abs() < f64::EPSILON {
        // 等温層。指数則。
        layer.base_pressure
            * (-STANDARD_GRAVITY * (geopotential - layer.base_altitude)
                / (SPECIFIC_GAS_CONSTANT * layer.base_temperature))
                .exp()
    } else {
        // 温度勾配のある層。べき乗則。
        let exponent = STANDARD_GRAVITY / (SPECIFIC_GAS_CONSTANT * layer.lapse_rate);
        layer.base_pressure * (layer.base_temperature / temperature).powf(exponent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 相対誤差での比較。リテラル同士の比較でも型が曖昧にならないよう注釈を付けている。
    macro_rules! assert_relative {
        ($actual:expr, $expected:expr, $relative_tolerance:expr) => {{
            let a: f64 = $actual;
            let e: f64 = $expected;
            let t: f64 = $relative_tolerance;
            assert!(
                (a - e).abs() <= e.abs() * t,
                "expected {a} ≈ {e} (within {}%), difference was {}",
                t * 100.0,
                (a - e).abs()
            );
        }};
    }

    // --- 公表されている標準値との照合 ---
    // 実装から導いた値ではなく、ICAO / U.S. Standard Atmosphere 1976 の表の値。

    #[test]
    fn sea_level_matches_isa_definition() {
        let s = Atmosphere::standard().sample(Meters(0.0));
        assert_relative!(s.temperature.get(), 288.15, 1e-9);
        assert_relative!(s.pressure.get(), 101_325.0, 1e-9);
        assert_relative!(s.density.get(), 1.225, 1e-3);
        assert_relative!(s.speed_of_sound.get(), 340.294, 1e-4);
        // 摂氏 15 度。
        assert_relative!(s.temperature.to_celsius(), 15.0, 1e-9);
    }

    #[test]
    fn tropopause_matches_isa_definition() {
        // 11 km（対流圏界面）: -56.5 °C、22 632 Pa。
        let s = Atmosphere::standard().sample(Meters(11_019.0));
        assert_relative!(s.temperature.to_celsius(), -56.5, 1e-3);
        assert_relative!(s.pressure.get(), 22_632.06, 1e-3);
    }

    #[test]
    fn standard_table_values_across_the_troposphere() {
        // ICAO 標準大気表より（幾何高度, 温度 `K`, 気圧 `Pa`）。
        let table = [
            (2_000.0, 275.15, 79_495.0),
            (5_000.0, 255.65, 54_020.0),
            (8_000.0, 236.15, 35_600.0),
            (10_000.0, 223.15, 26_436.0),
        ];
        for (altitude, temperature, pressure) in table {
            let s = Atmosphere::standard().sample(Meters(altitude));
            // 幾何/ジオポテンシャルの差があるため 0.5% の許容。
            assert_relative!(s.temperature.get(), temperature, 5e-3);
            assert_relative!(s.pressure.get(), pressure, 5e-3);
        }
    }

    #[test]
    fn cruise_altitude_density_is_about_a_third_of_sea_level() {
        // FL350（約 10 668 m）。ジェット旅客機の巡航高度。
        let s = Atmosphere::standard().sample(Meters(10_668.0));
        assert!(
            (0.30..0.40).contains(&s.density_ratio()),
            "density ratio at FL350 was {}, expected roughly 1/3",
            s.density_ratio()
        );
    }

    #[test]
    fn layer_boundaries_are_continuous() {
        // 各層の上端で計算した気圧・温度が、次の層の下端の公表値と一致すること。
        // ハードコードした境界値の転記ミスを検出する。
        for pair in LAYERS.windows(2) {
            let (lower, upper) = (pair[0], pair[1]);

            let temperature_at_top = lower.base_temperature
                + lower.lapse_rate * (upper.base_altitude - lower.base_altitude);
            assert_relative!(temperature_at_top, upper.base_temperature, 1e-9);

            let pressure_at_top = layer_pressure(lower, upper.base_altitude, temperature_at_top);
            assert_relative!(pressure_at_top, upper.base_pressure, 1e-4);
        }
    }

    #[test]
    fn geopotential_conversion_is_negligible_at_low_altitude_and_significant_high_up() {
        // 地表付近では幾何高度とほぼ等しい。
        assert_relative!(geopotential_altitude(100.0), 100.0, 1e-4);
        // 11 km では約 19 m 低くなる。この差を無視すると高高度で温度が 0.1 K ずれる。
        let difference = 11_000.0 - geopotential_altitude(11_000.0);
        assert!(
            (18.0..21.0).contains(&difference),
            "geopotential offset at 11 km was {difference} m, expected about 19 m"
        );
    }

    // --- 単調性 ---

    #[test]
    fn pressure_and_density_decrease_monotonically_with_altitude() {
        let atmosphere = Atmosphere::standard();
        let mut previous = atmosphere.sample(Meters(MIN_GEOMETRIC_ALTITUDE));

        for step in 1..=860 {
            let altitude = Meters(MIN_GEOMETRIC_ALTITUDE + f64::from(step) * 100.0);
            let current = atmosphere.sample(altitude);

            assert!(
                current.pressure.get() < previous.pressure.get(),
                "pressure increased with altitude at {altitude}"
            );
            assert!(
                current.density.get() < previous.density.get(),
                "density increased with altitude at {altitude}"
            );
            previous = current;
        }
    }

    // --- 非標準日 ---

    #[test]
    fn temperature_offset_shifts_temperature_and_density_but_not_pressure() {
        let standard = Atmosphere::standard().sample(Meters(1_500.0));
        let hot = Atmosphere::with_temperature_offset(20.0).sample(Meters(1_500.0));

        assert_relative!(
            hot.temperature.get(),
            standard.temperature.get() + 20.0,
            1e-9
        );
        // 気圧は変わらない。
        assert_relative!(hot.pressure.get(), standard.pressure.get(), 1e-12);
        // 暖かい空気は薄い。これが「暑い日は離陸滑走距離が伸びる」の物理的な理由。
        assert!(
            hot.density.get() < standard.density.get(),
            "a hotter day must produce thinner air"
        );
    }

    #[test]
    fn extreme_temperature_offset_does_not_produce_negative_density() {
        // 密度が負になると揚力の符号が反転し、機体が地面に吸い込まれる。
        // 天候システムが異常値を渡してきても、そこまで壊れないこと。
        for offset in [-1_000.0, -300.0, 300.0, 1_000.0] {
            let s = Atmosphere::with_temperature_offset(offset).sample(Meters(3_000.0));
            assert!(
                s.is_finite(),
                "offset {offset} produced a non-finite sample"
            );
            assert!(
                s.density.get() > 0.0,
                "offset {offset} produced density {}",
                s.density.get()
            );
            assert!(s.temperature.get() > 0.0);
            assert!(s.speed_of_sound.get() > 0.0);
        }
    }

    // --- 定義域外 ---

    #[test]
    fn out_of_range_altitudes_are_clamped_without_nan() {
        // 発散した機体が NaN を撒き散らすのを防ぐ。
        for altitude in [-1e9, -100_000.0, 1e6, 1e9, f64::MAX] {
            let s = Atmosphere::standard().sample(Meters(altitude));
            assert!(
                s.is_finite(),
                "altitude {altitude} produced a non-finite sample"
            );
            assert!(s.density.get() > 0.0);
        }
    }

    #[test]
    fn nan_altitude_does_not_propagate_into_the_sample() {
        // f64::clamp は NaN 入力に対して NaN を返すため、それだけでは守れない。
        let s = Atmosphere::standard().sample(Meters(f64::NAN));
        assert!(
            s.is_finite(),
            "a NaN altitude leaked into the atmosphere sample; \
             NaN propagates to every downstream state and makes debugging near-impossible"
        );
    }

    #[test]
    fn sampling_is_deterministic() {
        // ADR-0004 の不変条件。同じ入力からは常に同じ出力。
        let atmosphere = Atmosphere::with_temperature_offset(7.5);
        for altitude in [0.0, 1_234.5, 11_000.0, 30_000.0] {
            assert_eq!(
                atmosphere.sample(Meters(altitude)),
                atmosphere.sample(Meters(altitude))
            );
        }
    }

    #[test]
    fn mach_number_is_one_at_the_speed_of_sound() {
        let s = Atmosphere::standard().sample(Meters(0.0));
        assert_relative!(s.mach(s.speed_of_sound), 1.0, 1e-12);
    }
}
