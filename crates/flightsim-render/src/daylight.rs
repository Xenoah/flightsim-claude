//! 時刻の進行と、太陽高度に応じた光量。
//!
//! [`crate::sun`] が「太陽がどこにあるか」を決め、ここが「それをどう照らすか」を決める。
//!
//! # 光量と露出は組で決める
//!
//! **一度これで失敗している。** `FULL_DAYLIGHT`（2 万 lux）を
//! `Exposure::SUNLIGHT`（10 万 lux 級に合わせた露出）と組み合わせ、
//! 空だけ明るくて地面が真っ黒な絵になった。片方だけ触らないこと。
//!
//! # 減衰は GPU 側が既にやっている
//!
//! Bevy 0.18 の大気散乱は、**平行光源の色に透過率を掛けてから**地表の
//! シェーディングに使う（`bevy_pbr` の `pbr_lighting.wgsl` が
//! `sample_transmittance_lut` と `calculate_visible_sun_ratio` を掛けている）。
//! 空の色も同じ光源色に比例する。したがって:
//!
//! - 渡すべきは **大気圏外の照度**（`lux::RAW_SUNLIGHT` = 13 万 lux）。
//!   bevy の `atmosphere` の例も同じ理由でこの定数を使っている
//! - **ここで大気減衰を掛けると二重に減る。** 夕方の空が赤くなる前に黒くなる
//! - 地平線下の遮蔽も GPU 側が見ている。`calculate_visible_sun_ratio` は
//!   観測点の半径（＝高度）から地平線角を出すので、**上空で地平線が沈むぶん**まで
//!   正しく扱われる。こちらで仰角 0 を境に切ると、高高度の日没が早まる
//!
//! そのため既定は [`SunIlluminancePolicy::AboveAtmosphere`]。
//! `Atmosphere` を付けないカメラのために
//! [`SunIlluminancePolicy::Attenuated`] も用意してある。
//!
//! # 夜が来なかった原因は背景色だった
//!
//! **散乱の計算ではなく `ClearColor` の問題。** 大気散乱は
//! 「散乱光 + 背景 × 透過率」を書き出すので、bevy の既定の背景
//! （sRGB 43,44,47 の暗い灰色）が透過率ごしに空へ滲む。
//! `examples/sun_clock` で撮って画素を測ると:
//!
//! | 条件 | 空の平均画素 |
//! |---|---|
//! | 太陽高度 −16.9° | (40.3, 35.3, 27.9) |
//! | 太陽高度 −29.3° | (40.3, 35.3, 27.9) |
//! | **正午・照度 0 lux** | (40.3, 35.3, 27.9) |
//!
//! **太陽を消しても同じ値だった。** 空だと思っていたものは背景だった。
//! [`crate::FlightsimRenderPlugin`] が `ClearColor` を黒にしてこれを潰す。
//! 潰した後の同じ場面は (0.7, 0.7, 0.7)。
//!
//! ついでに分かったこと: **地平線より下に沈んだ太陽は空をほとんど照らさない。**
//! 太陽高度 −9.6° で照度を 2 倍にしても、空の画素の差は最大 1/255 だった。
//! bevy 0.18 の空は日没とともに急に暗くなり、薄明はごく短い。
//! ここで照度を細工しても直らないので、**環境光の側で薄明を作っている**。
//!
//! # 環境光
//!
//! 大気散乱は空と空気遠近法を描くが、**地表への回り込み（天空光）は落ちない**。
//! 影の中は平行光源が届かないので、環境光を入れないと影が真っ黒になる。
//! [`SunLighting::ambient`] が太陽高度で昼夜を混ぜる。
//!
//! 夜の値は **物理値ではない**。月の無い夜の地表は 0.001 lux 程度で、
//! 昼と同じ露出では完全な黒にしかならない。機体の輪郭が残る程度まで
//! 意図的に持ち上げてある。

use bevy::light::GlobalAmbientLight;
use bevy::light::light_consts::lux;
use bevy::prelude::*;
use flightsim_core::{Degrees, Radians, Seconds};

use crate::sun::{JulianDate, UtcDateTime, solar_position};
use crate::{CameraWorldPosition, SunDirection, sun_light_direction};

/// 大気圏外での太陽の直達照度。大気に濾される前の値。
///
/// 太陽定数 1361 W/m² に発光効率 約 93 lm/W を掛けた 12.7 万 lux 相当で、
/// bevy の `lux::RAW_SUNLIGHT` と同じ量。
pub const EXTRATERRESTRIAL_ILLUMINANCE: f32 = lux::RAW_SUNLIGHT;

/// 大気 1 気団あたりの消散係数（可視光の総和）。
///
/// 澄んだ大気での実測に基づく代表値。天頂で
/// `130 000 × exp(−0.21) ≈ 105 000 lux` となり、
/// 公表されている「快晴時の直達日射 約 10 万 lux」（bevy の
/// `lux::DIRECT_SUNLIGHT`）と一致する。
const EXTINCTION_PER_AIR_MASS: f64 = 0.21;

// ---------------------------------------------------------------------------
// 時刻
// ---------------------------------------------------------------------------

/// 時間の進み方。実時間 1 秒あたりのシミュレーション秒数。
///
/// **日の出を見るのに実時間で 6 時間待たせない。** 1x から 3600x まで。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct TimeRate(pub f64);

impl TimeRate {
    /// 停止。時刻が動かない。
    pub const PAUSED: Self = Self(0.0);
    /// 実時間と同じ速さ。
    pub const REAL_TIME: Self = Self(1.0);
    /// 1 日が 24 秒。空の変化を眺めるための最速。
    pub const FASTEST: Self = Self(3600.0);

    /// [`Self::faster`] / [`Self::slower`] が辿る段階。
    pub const PRESETS: [Self; 6] = [
        Self::PAUSED,
        Self::REAL_TIME,
        Self(10.0),
        Self(60.0),
        Self(600.0),
        Self::FASTEST,
    ];

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// 止まっているか。**NaN も停止として扱う。**
    ///
    /// `self.0 == 0.0` と書くと NaN が「動いている」側に落ち、
    /// 時刻が NaN になって太陽の位置ごと壊れる。
    /// 「速さが正である」ことを問い、その否定を取ること。
    #[must_use]
    pub fn is_paused(self) -> bool {
        !self.is_running()
    }

    /// 動いているか。NaN は「動いていない」。
    #[must_use]
    fn is_running(self) -> bool {
        self.0.abs() > 0.0
    }

    /// 次に速い段階。最速なら据え置き。
    #[must_use]
    pub fn faster(self) -> Self {
        Self::PRESETS
            .into_iter()
            .find(|preset| preset.0 > self.0)
            .unwrap_or(Self::FASTEST)
    }

    /// 次に遅い段階。停止なら据え置き。
    #[must_use]
    pub fn slower(self) -> Self {
        Self::PRESETS
            .into_iter()
            .rev()
            .find(|preset| preset.0 < self.0)
            .unwrap_or(Self::PAUSED)
    }

    /// 使える値に丸める。負・NaN は停止、上限は 1 日 4 秒。
    #[must_use]
    fn sanitised(self) -> Self {
        if self.is_paused() || !self.0.is_finite() {
            return Self::PAUSED;
        }
        Self(self.0.clamp(0.0, 21_600.0))
    }
}

impl Default for TimeRate {
    fn default() -> Self {
        Self::REAL_TIME
    }
}

/// シミュレーション内の現在時刻（UTC）。
///
/// **これが太陽の位置の唯一の入力。** 壁時計時間は見ない。
/// 同じ `TimeOfDay` を与えれば同じ光になるので、スクリーンショットの比較や
/// リプレイで絵が揃う。
#[derive(Resource, Debug, Clone, Copy)]
pub struct TimeOfDay {
    /// UTC のユリウス日。
    pub utc: JulianDate,
    /// 時間加速。
    pub rate: TimeRate,
}

impl Default for TimeOfDay {
    /// 2026-06-21 00:30 UTC。**東京の朝 9:30**（夏至）。
    ///
    /// この時刻・この地点で太陽は方位 104.0°（東南東）・仰角 58.8°。
    /// 影の向きが読め、地形の起伏が最も分かりやすい高さ。
    /// 起点が東京以外なら [`Self::at_local_mean_solar_time`] を使うこと。
    fn default() -> Self {
        Self::new(UtcDateTime::new(2026, 6, 21, 0, 30, 0.0).to_julian_date())
    }
}

impl TimeOfDay {
    #[must_use]
    pub fn new(utc: JulianDate) -> Self {
        Self {
            utc,
            rate: TimeRate::REAL_TIME,
        }
    }

    /// 指定した経度で地方平均太陽時が `civil` になる時刻。
    ///
    /// 「どこから始めても朝 9 時」を作る入口。時間帯も夏時間も見ない。
    #[must_use]
    pub fn at_local_mean_solar_time(civil: UtcDateTime, longitude: Radians) -> Self {
        Self::new(JulianDate::from_local_mean_solar_time(civil, longitude))
    }

    /// 実時間 `elapsed` ぶん進める。
    ///
    /// **壊れた値を時刻に入れない。** 一度 NaN になると太陽の位置も光量も
    /// 全部 NaN になり、原因の切り分けが極めて難しくなる。
    pub fn advance(&mut self, elapsed: Seconds) {
        if self.rate.is_paused() || !elapsed.get().is_finite() {
            return;
        }
        let advanced = self
            .utc
            .advanced_by(Seconds(elapsed.get() * self.rate.get()));
        if advanced.is_finite() {
            self.utc = advanced;
        }
    }

    /// 暦日と時刻。HUD の表示用。
    #[must_use]
    pub fn utc_date_time(self) -> UtcDateTime {
        self.utc.to_utc_date_time()
    }
}

// ---------------------------------------------------------------------------
// 光量
// ---------------------------------------------------------------------------

/// 平行光源に渡す照度をどう決めるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SunIlluminancePolicy {
    /// 大気圏外の照度をそのまま渡す。**カメラに `Atmosphere` がある場合はこれ。**
    ///
    /// 減衰・赤化・地平線下の遮蔽は GPU 側の透過率 LUT が行う。
    /// ここで減衰させると二重に掛かる。
    #[default]
    AboveAtmosphere,
    /// 太陽高度から大気減衰を掛けた地上の直達照度を渡す。
    ///
    /// `Atmosphere` を使わないカメラ向け。これが無いと、大気散乱を切った構成で
    /// 夜になっても地面が昼のまま照らされる。
    Attenuated,
}

/// 太陽光と環境光の設定。
#[derive(Resource, Debug, Clone, Copy)]
pub struct SunLighting {
    /// 照度の決め方。
    pub policy: SunIlluminancePolicy,
    /// 大気圏外の直達照度 lux。
    pub raw_illuminance: f32,
    /// 昼の環境光の輝度 cd/m²。**露出と組で決めること。**
    ///
    /// 快晴の青空の輝度が 数千 cd/m² 程度。影の中がこの明るさで持ち上がる。
    pub daylight_ambient: f32,
    /// 夜の環境光の輝度 cd/m²。**物理値ではない。**
    ///
    /// 真の夜空は昼と同じ露出では完全な黒になる。機体の輪郭が残る程度に
    /// 意図的に持ち上げた値。
    pub night_ambient: f32,
    /// 夜の環境光の色。青みを残すと「夜」に見える。
    pub night_tint: Color,
}

impl Default for SunLighting {
    fn default() -> Self {
        Self {
            policy: SunIlluminancePolicy::AboveAtmosphere,
            raw_illuminance: EXTRATERRESTRIAL_ILLUMINANCE,
            daylight_ambient: 6_000.0,
            night_ambient: 1_500.0,
            night_tint: Color::srgb(0.55, 0.65, 1.0),
        }
    }
}

impl SunLighting {
    /// 平行光源に渡す照度 lux。
    #[must_use]
    pub fn illuminance(&self, elevation: Radians) -> f32 {
        match self.policy {
            SunIlluminancePolicy::AboveAtmosphere => self.raw_illuminance,
            SunIlluminancePolicy::Attenuated => {
                direct_normal_illuminance(elevation, self.raw_illuminance)
            }
        }
    }

    /// 環境光。昼夜を太陽高度で混ぜる。
    ///
    /// 混ぜ方は 2 段。
    ///
    /// 1. 薄明の橋渡し（地平線下 6° 〜 地平線上 6° で 0 → 1）
    /// 2. さらに `sqrt(sin h)` を掛けて、低い太陽では天空光も暗くする。
    ///    快晴時の天空光の水平面照度は太陽高度とともに落ちる。これを掛けないと、
    ///    **日没直前の日陰が真昼と同じ明るさで残る**
    #[must_use]
    pub fn ambient(&self, elevation: Radians) -> GlobalAmbientLight {
        // **2 つの係数は役割が違う。単純に掛けると薄明が消える。**
        //
        // `skylight_fraction` は地平線で 0 になるので、そのまま掛けると
        // 日没の瞬間に夜の下限へ落ちる。`daylight_fraction` が -6°..+6° を
        // 覆うよう作られているのに、その区間が丸ごと潰れていた
        // （実測: 高度 -0.18° で既に夜と同じ 1500、地形の画素値 3〜7/255）。
        //
        // 薄明の幅は `daylight_fraction` が決め、`skylight_fraction` は
        // 「太陽が高いほど明るい」を地平線より上で足すだけにする。
        // 下限 0.3 は、地平線上の太陽でも天空光が消えないことを表す。
        let day = daylight_fraction(elevation) * (0.3 + 0.7 * skylight_fraction(elevation));
        let brightness = self.night_ambient + (self.daylight_ambient - self.night_ambient) * day;

        let night = self.night_tint.to_linear();
        let color = Color::linear_rgb(
            night.red + (1.0 - night.red) * day,
            night.green + (1.0 - night.green) * day,
            night.blue + (1.0 - night.blue) * day,
        );

        GlobalAmbientLight {
            color,
            brightness: brightness.max(0.0),
            affects_lightmapped_meshes: true,
        }
    }
}

/// 昼の度合い。地平線下 6°（市民薄明の下限）で 0、地平線上 6° で 1。
///
/// 端で滑らかに繋がる曲線を使う。線形だと薄明の入りと終わりに折れ目が見える。
#[must_use]
fn daylight_fraction(elevation: Radians) -> f32 {
    let degrees = elevation.to_degrees().get();
    if !degrees.is_finite() {
        return 0.0;
    }
    let t = ((degrees + 6.0) / 12.0).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, reason = "0..=1 の比率。f32 で十分")]
    let t = t as f32;
    // smoothstep。両端で傾きが 0 になる。
    t * t * (3.0 - 2.0 * t)
}

/// 天空光の強さ。太陽が天頂にあるとき 1、地平線で 0。
///
/// `sqrt(sin h)`。快晴時の天空光（拡散日射）の水平面照度は、直達日射ほど
/// 急ではないが太陽高度とともに落ちる。**平方根はその緩やかさを表す経験則**で、
/// 厳密な放射計算ではない。線形にすると夕方の日陰が暗くなりすぎ、
/// 一定にすると明るすぎる。
#[must_use]
fn skylight_fraction(elevation: Radians) -> f32 {
    let sin_h = elevation.sin();
    if !sin_h.is_finite() {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation, reason = "0..=1 の比率。f32 で十分")]
    let value = sin_h.clamp(0.0, 1.0).sqrt() as f32;
    value
}

/// 太陽高度から地上の直達法線面照度（lux）を求める。
///
/// # 根拠
///
/// Beer–Lambert 則 `E = E₀ exp(−τ·m)`。気団 `m` は Kasten & Young (1989) の
/// 近似式で、地平線付近まで使える（真の相対気団は地平線で約 38）。
///
/// ```text
///   m = 1 / (sin h + 0.50572 (h° + 6.07995)^−1.6364)
/// ```
///
/// 天頂で 10.5 万 lux、仰角 30° で 8.6 万 lux、仰角 10° で 4.0 万 lux。
/// 公表されている快晴時の直達日射（天頂で約 10 万 lux）と桁も値も一致する。
///
/// **仰角が 0 以下なら 0。** 地平線直下からの直達日射は無い。
/// 実際には地平線での値が 45 lux 程度なので、そこでの不連続は
/// 全体の 0.05% に満たない。
#[must_use]
pub fn direct_normal_illuminance(elevation: Radians, extraterrestrial: f32) -> f32 {
    let degrees = elevation.to_degrees().get();
    if !degrees.is_finite() || degrees <= 0.0 {
        return 0.0;
    }
    let sin_h = Degrees(degrees).to_radians().sin();
    let denominator = sin_h + 0.50572 * (degrees + 6.079_95).powf(-1.6364);
    if denominator <= 0.0 {
        return 0.0;
    }
    let air_mass = 1.0 / denominator;
    let attenuation = (-EXTINCTION_PER_AIR_MASS * air_mass).exp();

    #[allow(
        clippy::cast_possible_truncation,
        reason = "照度は 13 万 lux 以下。f32 の精度で十分"
    )]
    let value = (f64::from(extraterrestrial) * attenuation) as f32;
    value.max(0.0)
}

// ---------------------------------------------------------------------------
// ECS
// ---------------------------------------------------------------------------

/// 太陽の平行光源。**この印を付けた `DirectionalLight` が時刻に追随する。**
///
/// 付いていない光源は触らない。夜間の着陸灯のような、時刻と無関係の光を
/// 後から足せるようにするため。
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct SunLight;

/// 実時間ぶん時刻を進める。
pub fn advance_time_of_day(time: Res<Time>, mut clock: ResMut<TimeOfDay>) {
    // 停止中は `ResMut` を触らない。毎フレーム変更通知を出すと、
    // `Changed<TimeOfDay>` を見る下流（HUD など）が無駄に走る。
    if clock.bypass_change_detection().rate.sanitised().is_paused() {
        return;
    }
    clock.rate = clock.rate.sanitised();
    clock.advance(Seconds(f64::from(time.delta_secs())));
}

/// 時刻とカメラ位置から太陽の方位・仰角を求める。
///
/// **観測地点はカメラ。** 太陽の向き自体（ECEF）は地球上のどこでも同じだが、
/// 方位角と仰角は**その地点の地平面**を基準に決まるので観測地点に依存する。
/// 50 km 離れれば仰角が 0.45° 変わる（地球の中心角ぶん）。
/// 絵の陰影を決めるのはカメラの位置なので、カメラを使う。
pub fn update_sun_direction(
    clock: Res<TimeOfDay>,
    camera: Res<CameraWorldPosition>,
    mut sun: ResMut<SunDirection>,
) {
    let position = solar_position(clock.utc, camera.0);
    *sun = SunDirection::from(position);
}

/// 太陽の向きと光量を `DirectionalLight` と環境光へ反映する。
pub fn apply_sun_light(
    sun: Res<SunDirection>,
    lighting: Res<SunLighting>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut lights: Query<(&mut DirectionalLight, &mut Transform), With<SunLight>>,
) {
    let direction = sun_light_direction(*sun);
    let illuminance = lighting.illuminance(sun.elevation);
    for (mut light, mut transform) in &mut lights {
        light.illuminance = illuminance;
        transform.look_to(direction, Vec3::Y);
    }
    *ambient = lighting.ambient(sun.elevation);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elevation(degrees: f64) -> Radians {
        Degrees(degrees).to_radians()
    }

    // --- 時間の進行 ---

    #[test]
    fn real_time_advances_one_second_per_second() {
        let mut clock = TimeOfDay::new(UtcDateTime::midnight(2026, 6, 21).to_julian_date());
        let start = clock.utc.get();
        clock.advance(Seconds(60.0));
        let elapsed_seconds = (clock.utc.get() - start) * 86_400.0;
        // **ユリウス日を f64 で持つ以上、刻みは 40 µs 程度が限界**
        // （2.46e6 日の f64 の刻みが 4.6e-10 日）。太陽は 1 秒で 0.004° しか
        // 動かないので、この粗さは見た目に出ない。1 ms を上限として固定しておく。
        assert!(
            (elapsed_seconds - 60.0).abs() < 1e-3,
            "60 real seconds should be 60 simulated seconds, got {elapsed_seconds}"
        );
    }

    #[test]
    fn time_acceleration_multiplies_the_elapsed_time() {
        // 3600x なら 24 秒で 1 日。**日の出を待たずに見られること。**
        let mut clock = TimeOfDay::new(UtcDateTime::midnight(2026, 6, 21).to_julian_date());
        clock.rate = TimeRate::FASTEST;
        let start = clock.utc.get();
        for _ in 0..(24 * 60) {
            clock.advance(Seconds(1.0 / 60.0));
        }
        let days = clock.utc.get() - start;
        assert!(
            (days - 1.0).abs() < 1e-6,
            "24 s at 3600x should be one day, got {days:.6} days"
        );
    }

    #[test]
    fn a_paused_clock_does_not_move() {
        let mut clock = TimeOfDay::new(UtcDateTime::midnight(2026, 6, 21).to_julian_date());
        clock.rate = TimeRate::PAUSED;
        let start = clock.utc;
        clock.advance(Seconds(3600.0));
        assert!((clock.utc.get() - start.get()).abs() < f64::EPSILON);
    }

    #[test]
    fn the_clock_refuses_broken_input() {
        // **NaN が一度でも入ると太陽の位置ごと壊れる。**
        let mut clock = TimeOfDay::new(UtcDateTime::midnight(2026, 6, 21).to_julian_date());
        let start = clock.utc.get();
        clock.advance(Seconds(f64::NAN));
        clock.advance(Seconds(f64::INFINITY));
        clock.rate = TimeRate(f64::NAN);
        clock.advance(Seconds(1.0));
        assert!(
            (clock.utc.get() - start).abs() < f64::EPSILON,
            "broken input moved the clock to {}",
            clock.utc.get()
        );
        assert!(clock.utc.is_finite());
    }

    #[test]
    fn the_rate_steps_through_the_presets_and_stops_at_the_ends() {
        let mut rate = TimeRate::PAUSED;
        for _ in 0..10 {
            rate = rate.faster();
        }
        assert!(
            (rate.get() - TimeRate::FASTEST.get()).abs() < f64::EPSILON,
            "speeding up should stop at the fastest preset, got {rate:?}"
        );
        for _ in 0..10 {
            rate = rate.slower();
        }
        assert!(rate.is_paused(), "slowing down should stop at pause");

        // 段階が単調であること。順番が入れ替わると押した向きと逆に動く。
        for pair in TimeRate::PRESETS.windows(2) {
            assert!(pair[0].get() < pair[1].get(), "the presets are not sorted");
        }
    }

    #[test]
    fn a_broken_rate_is_treated_as_paused() {
        assert!(TimeRate(f64::NAN).is_paused());
        assert!(TimeRate(-1.0).sanitised().is_paused());
        assert!(TimeRate(f64::INFINITY).sanitised().get() <= 21_600.0);
    }

    #[test]
    fn starting_at_local_noon_puts_the_sun_high_wherever_you_are() {
        // どこから始めても同じ時間帯で始められること。
        for longitude in [-150.0, -75.0, 0.0, 100.0, 139.78, 179.0] {
            let observer = flightsim_core::Geodetic::from_degrees(35.0, longitude, 0.0);
            let clock = TimeOfDay::at_local_mean_solar_time(
                UtcDateTime::new(2026, 6, 21, 12, 0, 0.0),
                observer.longitude,
            );
            let position = solar_position(clock.utc, observer);
            let degrees = position.elevation.to_degrees().get();
            assert!(
                degrees > 70.0,
                "local noon at {longitude}° gave an elevation of {degrees:.1}°"
            );
        }
    }

    // --- 光量 ---

    #[test]
    fn the_zenith_sun_matches_the_published_direct_sunlight_figure() {
        // 快晴時の直達日射は約 10 万 lux（bevy の lux::DIRECT_SUNLIGHT と同じ）。
        let value = direct_normal_illuminance(elevation(90.0), EXTRATERRESTRIAL_ILLUMINANCE);
        let published = lux::DIRECT_SUNLIGHT;
        assert!(
            (value - published).abs() < published * 0.1,
            "the zenith sun should be within 10% of {published} lux, got {value:.0}"
        );
    }

    #[test]
    fn the_illuminance_falls_off_toward_the_horizon() {
        // 気団が増えるほど暗くなること。逆転していると夕方が真昼より明るくなる。
        let mut previous = f32::INFINITY;
        for degrees in [90.0, 60.0, 30.0, 20.0, 10.0, 5.0, 1.0, 0.1] {
            let value = direct_normal_illuminance(elevation(degrees), EXTRATERRESTRIAL_ILLUMINANCE);
            assert!(
                value < previous,
                "{degrees}° gave {value:.0} lux, which is not below {previous:.0}"
            );
            assert!(value >= 0.0);
            previous = value;
        }
    }

    #[test]
    fn the_low_sun_matches_published_clear_sky_figures() {
        // 快晴時の直達法線面照度の目安（仰角 30° で 8〜9 万 lux、
        // 仰角 10° で 4〜5 万 lux）。
        let thirty = direct_normal_illuminance(elevation(30.0), EXTRATERRESTRIAL_ILLUMINANCE);
        assert!(
            (80_000.0..90_000.0).contains(&thirty),
            "30° should give 80–90 klux, got {thirty:.0}"
        );
        let ten = direct_normal_illuminance(elevation(10.0), EXTRATERRESTRIAL_ILLUMINANCE);
        assert!(
            (40_000.0..50_000.0).contains(&ten),
            "10° should give 40–50 klux, got {ten:.0}"
        );
    }

    #[test]
    fn the_sun_below_the_horizon_does_not_light_anything() {
        for degrees in [-0.1, -6.0, -30.0, -90.0] {
            let value = direct_normal_illuminance(elevation(degrees), EXTRATERRESTRIAL_ILLUMINANCE);
            assert!(
                value.abs() < f32::EPSILON,
                "a sun {degrees}° below the horizon lit the ground with {value} lux"
            );
        }
    }

    #[test]
    fn the_default_policy_hands_over_the_unfiltered_illuminance() {
        // **大気散乱が減衰を担当している。** ここで減らすと二重に掛かり、
        // 夕方の空が赤くなる前に黒くなる。地平線の下でも触らないこと
        // （高度を上げると地平線は沈み、仰角が負でも太陽はまだ見える）。
        let lighting = SunLighting::default();
        for degrees in [90.0, 30.0, 1.0, 0.0, -3.0, -20.0] {
            let value = lighting.illuminance(elevation(degrees));
            assert!(
                (value - lux::RAW_SUNLIGHT).abs() < f32::EPSILON,
                "{degrees}° gave {value} lux instead of the raw value"
            );
        }
    }

    #[test]
    fn the_attenuated_policy_follows_the_sun() {
        let lighting = SunLighting {
            policy: SunIlluminancePolicy::Attenuated,
            ..SunLighting::default()
        };
        assert!(lighting.illuminance(elevation(60.0)) > lighting.illuminance(elevation(10.0)));
        assert!(lighting.illuminance(elevation(-10.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn the_night_is_dark_but_not_invisible() {
        // **真っ暗にすると機体が消える。** 夜でも輪郭が残ること。
        let lighting = SunLighting::default();
        let night = lighting.ambient(elevation(-30.0));
        assert!(
            night.brightness > 0.0,
            "the night ambient must not be exactly zero"
        );
        assert!(
            night.brightness < lighting.daylight_ambient * 0.5,
            "the night must be clearly darker than the day, got {}",
            night.brightness
        );
        // 昼は白、夜は青み。青が赤より強いこと。
        let tint = night.color.to_linear();
        assert!(
            tint.blue > tint.red,
            "the night ambient should stay blue, got {tint:?}"
        );
    }

    #[test]
    fn the_ambient_light_rises_with_the_sun() {
        let lighting = SunLighting::default();
        let mut previous = f32::NEG_INFINITY;
        for degrees in [-30.0, -6.0, -3.0, 0.0, 3.0, 6.0, 45.0, 90.0] {
            let brightness = lighting.ambient(elevation(degrees)).brightness;
            assert!(
                brightness >= previous,
                "{degrees}° gave {brightness}, below the previous {previous}"
            );
            previous = brightness;
        }
        assert!(
            (previous - lighting.daylight_ambient).abs() < f32::EPSILON,
            "the high sun should reach the full daylight ambient, got {previous}"
        );
    }

    #[test]
    fn the_twilight_blend_has_no_step_in_it() {
        // 段差があると、日没の瞬間に画面全体の明るさが跳ぶ。
        let lighting = SunLighting::default();
        let mut previous = lighting.ambient(elevation(-20.0)).brightness;
        let mut degrees = -20.0;
        while degrees <= 20.0 {
            let brightness = lighting.ambient(elevation(degrees)).brightness;
            assert!(
                (brightness - previous).abs() < lighting.daylight_ambient * 0.05,
                "the ambient jumps by {} at {degrees}°",
                (brightness - previous).abs()
            );
            previous = brightness;
            degrees += 0.25;
        }
    }

    #[test]
    fn broken_elevations_do_not_produce_broken_light() {
        let lighting = SunLighting::default();
        for degrees in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e9] {
            let ambient = lighting.ambient(elevation(degrees));
            assert!(
                ambient.brightness.is_finite() && ambient.brightness >= 0.0,
                "{degrees}° gave an ambient of {}",
                ambient.brightness
            );
            let illuminance = direct_normal_illuminance(
                elevation(degrees),
                SunLighting::default().raw_illuminance,
            );
            assert!(illuminance.is_finite() && illuminance >= 0.0);
        }
    }
}
