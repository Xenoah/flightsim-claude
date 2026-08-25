//! 突風と乱流。
//!
//! # 乱数を使わない
//!
//! FDM は決定論的でなければならない（[ADR-0004](../../../../docs/adr/0004-simulation-loop.md)、
//! CLAUDE.md 規約 6）。壁時計・乱数・グローバル可変状態は禁止で、
//! **同じ入力なら必ず同じ出力**でなければリプレイもネットワーク同期も成立しない。
//!
//! そこで乱流を「乱数」ではなく**時刻と位置の決定論的な関数**として作る。
//! 値ノイズ（格子点のハッシュ値を滑らかに補間する）を使い、シードから
//! 全てが決まるようにしてある。
//!
//! # 1 ステップごとの独立な乱数では「揺れ」にならない
//!
//! 毎ステップ独立に値を引くと、120 Hz で符号が反転する力になる。
//! これは物理的にありえないし、体感は「揺れ」ではなく「痙攣」になる。
//!
//! 空間相関長と時間相関を明示的な定数として持ち、**滑らかに変化する場**を
//! 作る。機体はその場の中を移動するので、速く飛べば速く揺れる（実機と同じ）。
//!
//! # 強度の目安
//!
//! 航空気象の慣習に倣い、RMS 風速（m/s）で段階を決める。
//! 実機の目安は Light が 1〜2、Moderate が 2〜4、Severe が 4 以上。

use crate::Environment;
use flightsim_core::{Geodetic, MetersPerSecond, Ned, Seconds};

/// 空間相関長。この距離だけ離れると擾乱がほぼ無相関になる。
///
/// 大気境界層の渦の代表寸法。**短くすると痙攣し、長くするとうねりになる。**
/// 巡航 50 m/s なら 150 m を 3 秒で通り、体感の周期は数秒になる。
const CORRELATION_LENGTH: f64 = 150.0;

/// 時間相関の尺度。渦そのものが変形・移流していく速さ。
///
/// 空間相関だけだと、静止した機体がまったく揺れない。
const CORRELATION_TIME: f64 = 4.0;

/// 縦（上下）成分の強さを、水平成分に対する比で表したもの。
///
/// **体感上いちばん効くのは上下の揺れ。** 実大気では等方に近いが、
/// 上下は迎角を直接変えるため、同じ強度でも機体の反応が大きい。
/// ここを 1.0 より下げてあるのは、等方にすると上下が支配的になりすぎて
/// 操縦が成立しなくなったため（体感の調整であり、気象の実測値ではない）。
const VERTICAL_RATIO: f64 = 0.7;

/// 乱流の強さ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turbulence {
    /// 擾乱の RMS 風速。0 なら無乱流。
    pub intensity: MetersPerSecond,
    /// 場を決める種。**変えると別の大気になる。**
    pub seed: u64,
}

impl Default for Turbulence {
    fn default() -> Self {
        Self::CALM
    }
}

impl Turbulence {
    /// 無乱流。
    pub const CALM: Self = Self {
        intensity: MetersPerSecond(0.0),
        seed: 0,
    };

    /// 軽い乱流。巡航中に軽く小突かれる程度。
    #[must_use]
    pub const fn light(seed: u64) -> Self {
        Self {
            intensity: MetersPerSecond(1.5),
            seed,
        }
    }

    /// 中程度。着陸で当て舵が要る。
    #[must_use]
    pub const fn moderate(seed: u64) -> Self {
        Self {
            intensity: MetersPerSecond(3.0),
            seed,
        }
    }

    /// 激しい乱流。姿勢を保つのが難しい。
    #[must_use]
    pub const fn severe(seed: u64) -> Self {
        Self {
            intensity: MetersPerSecond(6.0),
            seed,
        }
    }

    /// 揺れているか。無乱流なら計算を丸ごと省ける。
    ///
    /// **NaN も「無乱流」として扱う。** 壊れた強度で場を計算すると
    /// NaN が大気へ入り、全状態へ伝播する。
    #[must_use]
    pub fn is_calm(self) -> bool {
        let intensity = self.intensity.get();
        !intensity.is_finite() || intensity <= 0.0
    }

    /// この時刻・この位置での擾乱。定常風に**加える**ローカル NED ベクトル。
    ///
    /// 同じ引数なら必ず同じ値を返す。非有限な入力では無風を返す
    /// （**NaN を大気へ入れると全状態へ伝播する**）。
    #[must_use]
    pub fn gust_at(self, elapsed: Seconds, position: Geodetic) -> Ned {
        let intensity = self.intensity.get();
        if !intensity.is_finite() || intensity <= 0.0 {
            return Ned::new(0.0, 0.0, 0.0);
        }

        let time = elapsed.get();
        let latitude = position.latitude.get();
        let longitude = position.longitude.get();
        let altitude = position.altitude.get();
        if !(time.is_finite()
            && latitude.is_finite()
            && longitude.is_finite()
            && altitude.is_finite())
        {
            return Ned::new(0.0, 0.0, 0.0);
        }

        // 測地座標をおおよそのメートルへ。**厳密な変換は要らない**
        // （乱流の場は元より恣意的で、必要なのは滑らかさと相関長だけ）。
        // 緯度 1 rad ≒ 6.37e6 m、経度は cos(緯度) 倍。
        const EARTH_RADIUS: f64 = 6_371_000.0;
        let north = latitude * EARTH_RADIUS / CORRELATION_LENGTH;
        let east = longitude * EARTH_RADIUS * latitude.cos() / CORRELATION_LENGTH;
        let up = altitude / CORRELATION_LENGTH;
        let phase = time / CORRELATION_TIME;

        // 3 軸それぞれ別の種で引く。同じ種だと 3 成分が同位相になり、
        // 揺れが一直線になる。
        let horizontal = intensity;
        let vertical = intensity * VERTICAL_RATIO;
        Ned::new(
            horizontal * noise4(self.seed ^ 0x9E37_79B9_7F4A_7C15, north, east, up, phase),
            horizontal * noise4(self.seed ^ 0xBF58_476D_1CE4_E5B9, north, east, up, phase),
            vertical * noise4(self.seed ^ 0x94D0_49BB_1331_11EB, north, east, up, phase),
        )
    }
}

/// 4 次元の値ノイズ。おおよそ `[-1, 1]`。
///
/// 格子点のハッシュ値を smoothstep で補間する。**同じ座標なら必ず同じ値**で、
/// 隣接点では滑らかに繋がる。
fn noise4(seed: u64, x: f64, y: f64, z: f64, w: f64) -> f64 {
    let (xi, xf) = split(x);
    let (yi, yf) = split(y);
    let (zi, zf) = split(z);
    let (wi, wf) = split(w);

    let (sx, sy, sz, sw) = (
        smoothstep(xf),
        smoothstep(yf),
        smoothstep(zf),
        smoothstep(wf),
    );

    let mut total = 0.0;
    for corner in 0..16_u32 {
        let dx = i64::from(corner & 1);
        let dy = i64::from((corner >> 1) & 1);
        let dz = i64::from((corner >> 2) & 1);
        let dw = i64::from((corner >> 3) & 1);

        let weight = blend(sx, dx) * blend(sy, dy) * blend(sz, dz) * blend(sw, dw);
        if weight == 0.0 {
            continue;
        }
        total += weight * lattice(seed, xi + dx, yi + dy, zi + dz, wi + dw);
    }
    total
}

/// 整数部と小数部に分ける。負の座標でも小数部が `[0, 1)` に収まるようにする。
fn split(value: f64) -> (i64, f64) {
    let floor = value.floor();
    // 極端な座標でも i64 の範囲に収める。地球規模の座標は十分収まる。
    #[allow(
        clippy::cast_possible_truncation,
        reason = "floor をクランプ済み。i64 の範囲に収まる"
    )]
    let index = floor.clamp(-1.0e15, 1.0e15) as i64;
    (index, value - floor)
}

/// 端で微分が 0 になる補間。線形にすると格子が縞として見える。
fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blend(smoothed: f64, corner: i64) -> f64 {
    if corner == 0 {
        1.0 - smoothed
    } else {
        smoothed
    }
}

/// 格子点の値。`[-1, 1]`。
fn lattice(seed: u64, x: i64, y: i64, z: i64, w: i64) -> f64 {
    // SplitMix64 の混合。安価で、隣接する整数を十分に散らす。
    let mut hash = seed;
    for component in [x, y, z, w] {
        #[allow(
            clippy::cast_sign_loss,
            reason = "ハッシュのためのビット再解釈。値の大小に意味はない"
        )]
        let bits = component as u64;
        hash = hash.wrapping_add(bits).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        hash ^= hash >> 31;
        hash = hash.wrapping_mul(0x94D0_49BB_1331_11EB);
        hash ^= hash >> 29;
    }
    // 上位 53 ビットを [0, 1) にしてから [-1, 1] へ。
    #[allow(clippy::cast_precision_loss, reason = "53 ビットは f64 の仮数に収まる")]
    let unit = (hash >> 11) as f64 / (1_u64 << 53) as f64;
    unit.mul_add(2.0, -1.0)
}

impl Environment {
    /// 乱流を加えた環境を作る。
    ///
    /// 擾乱はローカル NED で計算し、既存の `wind_ecef` へ足す。
    /// **`Environment` は `Copy` のまま**で、1 ステップの間は固定される
    /// （ADR-0004: 接地平面と同じく、RK4 の中間評価で値が変わらないこと）。
    #[must_use]
    pub fn with_turbulence(
        mut self,
        turbulence: Turbulence,
        elapsed: Seconds,
        position: Geodetic,
    ) -> Self {
        if turbulence.is_calm() {
            return self;
        }
        let gust = turbulence.gust_at(elapsed, position);
        let gust_ecef = flightsim_core::LocalFrame::new(position).ned_to_ecef_vector(gust);
        if gust_ecef.is_finite() {
            self.wind_ecef += gust_ecef;
        }
        self
    }
}
