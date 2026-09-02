//! # flightsim-audio
//!
//! エンジン音・風切り音・失速警報。**波形は同梱ファイルでも録音でもなく、
//! 毎標本その場で合成する**（[ADR-0009]）。
//!
//! ## なぜ音が要るのか
//!
//! 失速が近いことを知る手段が、これが無いと**計器を見ることしかない**。
//! 着陸の最終段階で計器を見ている余裕は無いので、実機には必ず失速警報が
//! 付いている。エンジン音と風切り音も同じで、出力と速度を耳で追えると、
//! 視線を外に置いたまま機体の状態が分かる。
//!
//! ## どう作っているか
//!
//! **エンジンの音は、持続する倍音ではなく排気の圧力パルスの連なり**である。
//! 正弦波を足して作ると、周波数が合っていても「オルガン」にしかならない
//! （最初にそう作って、実際そうなった）。
//!
//! 今の作りは Pulse-Train-Resonator: 点火のたびにパルスを立て、それを
//! 排気管と機体の共鳴に通す。**回転数はパルスの間隔だけを変え、共鳴は
//! その場に留まる。** 動く倍音列と動かない共鳴の組み合わせが「物体が
//! 鳴っている」という印象を作る。詳細は [`engine`] を参照。
//!
//! | モジュール | 役割 |
//! |---|---|
//! | [`dsp`] | フィルタ・共鳴・雑音・平滑化。1 標本ずつ処理する部品 |
//! | [`engine`] | ピストンエンジンとプロペラ。点火周波数と BPF は諸元から出す |
//! | [`airframe`] | 風切り音と失速警報 |
//! | [`mixer`] | 3 つを混ぜて 1 本にする。ここまで Bevy 非依存 |
//! | [`source`] | Bevy（rodio）へ流す。音の thread とは原子変数で受け渡す |
//!
//! ## 構成
//!
//! ```text
//!             app (Bevy)
//!                 │
//!   render / input / ui / audio   ← Bevy 依存層。互いに依存しない
//!                 │
//!                sim
//! ```
//!
//! [`FlightAudioPlugin`] を足し、毎フレーム [`AircraftSound`] を埋める。
//! **app が埋めなければ何も鳴らない**ので、音を出さない構成でも動く。
//!
//! [ADR-0009]: https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0009-synthesised-audio.md

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム。参照に変えると system として登録できない"
)]
// このクレートは至るところで「標本数 ↔ 秒」「標本の位置 ↔ 位相」を行き来する。
// 扱う標本数は 48 kHz × 数十秒（せいぜい 10^6）で、f64 の仮数 52 bit にも
// usize にも大きく余裕がある。**音を作る値であって物理量ではない**ので、
// ここでの丸めは可聴域に影響しない。
//
// 物理量の単位取り違えを型で潰す方針（CLAUDE.md）は `flightsim-core` の
// newtype が担っており、そちらは変わらない。
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "標本数と位相の変換。48 kHz × 数十秒は f64 にも usize にも収まる"
)]

use std::sync::Arc;

use bevy::audio::AddAudioSource;
use bevy::prelude::*;

use flightsim_core::MetersPerSecond;

pub mod airframe;
pub mod dsp;
pub mod engine;
pub mod mixer;
pub mod source;

pub use engine::{EngineSpec, estimate_rpm};
pub use mixer::{DEFAULT_MASTER, FlightSound};
pub use source::{FlightAudio, SharedSound};

/// 機体が今どういう音を出しているか。app が毎フレーム埋める。
///
/// **物理量で渡す。** ここで音量や周波数を受け取る形にすると、
/// 「どういう音にするか」の判断が app に散る。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct AircraftSound {
    /// 出力 `[0, 1]`。エンジンの回転数と音色を決める。
    pub throttle: f64,
    /// 対気速度。風切り音の大きさと明るさを決める。
    pub airspeed: MetersPerSecond,
    /// 失速警報を鳴らすか。
    pub stall_warning: bool,
    /// 音を止めるか。一時停止・墜落で立てる。
    pub muted: bool,
}

impl AircraftSound {
    /// 合成側へ渡す形にする。
    #[must_use]
    pub const fn to_flight_sound(self) -> FlightSound {
        FlightSound {
            throttle: self.throttle,
            airspeed: self.airspeed.get(),
            stall_warning: self.stall_warning,
            muted: self.muted,
        }
    }
}

/// 音の設定。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct AudioSettings {
    /// 全体の音量 `[0, 1]`。
    pub master: f64,
    /// 音を出すか。偽なら音源自体を作らない。
    pub enabled: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master: DEFAULT_MASTER,
            enabled: true,
        }
    }
}

/// 音の thread と共有する状態への取っ手。
///
/// **`Arc` で持つ。** 音は別の thread で作られるので、ECS 側と同じものを
/// 指す必要がある。
#[derive(Resource, Debug, Clone)]
pub struct SoundBridge(pub Arc<SharedSound>);

/// 音を鳴らすプラグイン。
#[derive(Debug, Default)]
pub struct FlightAudioPlugin;

impl Plugin for FlightAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<FlightAudio>()
            .init_resource::<AircraftSound>()
            .init_resource::<AudioSettings>()
            .insert_resource(SoundBridge(Arc::new(SharedSound::default())))
            .add_systems(Startup, spawn_sound_source)
            .add_systems(Update, (publish_sound, report_playback));
    }
}

/// 音源を 1 つ置いて、鳴らし始める。
///
/// **1 本にまとめてある。** 音源ごとに分けると、混ぜるのが出力機器側の
/// 仕事になり、こちらで頭を抑えられない（3 つ足して 1 を超えたときに割れる）。
pub fn spawn_sound_source(
    mut commands: Commands,
    settings: Res<AudioSettings>,
    bridge: Res<SoundBridge>,
    mut sources: ResMut<Assets<FlightAudio>>,
) {
    if !settings.enabled {
        info!("audio: disabled");
        return;
    }
    let spec = EngineSpec::default();
    let handle = sources.add(FlightAudio::new(Arc::clone(&bridge.0), spec));
    bridge.0.set_master(settings.master);

    // 諸元と、そこから出る周波数を出す。**「音が違う」と思ったとき、
    // 諸元が意図どおりかをまず確かめられる。**
    info!(
        "audio: {} cylinder {}-stroke, {} blade prop, {:.0}-{:.0} rpm \
         (firing {:.0}-{:.0} Hz, blade passage {:.0}-{:.0} Hz)",
        spec.cylinders,
        spec.strokes,
        spec.blades,
        spec.idle_rpm,
        spec.max_rpm,
        spec.firing_hz(spec.idle_rpm),
        spec.firing_hz(spec.max_rpm),
        spec.blade_passage_hz(spec.idle_rpm),
        spec.blade_passage_hz(spec.max_rpm),
    );

    commands.spawn((
        AudioPlayer(handle),
        // 終わらない音源なので、繰り返しの設定は要らない。
        PlaybackSettings::ONCE,
        Name::new("flight audio"),
    ));
}

/// 機体の状態を音の thread へ渡す。
pub fn publish_sound(
    sound: Res<AircraftSound>,
    settings: Res<AudioSettings>,
    bridge: Res<SoundBridge>,
) {
    bridge.0.set(sound.to_flight_sound());
    if settings.is_changed() {
        bridge.0.set_master(if settings.enabled {
            settings.master
        } else {
            0.0
        });
    }
}

/// 再生が始まったことを一度だけ出す。
///
/// **`AudioSink` は再生が始まって初めて付く。** これが出ないなら、
/// 音源は作れたが鳴っていない（出力機器が無い、など）。無音の原因を
/// 「合成が悪いのか、再生に届いていないのか」に切り分けられる。
pub fn report_playback(sinks: Query<Entity, Added<AudioSink>>) {
    for _ in &sinks {
        info!("audio: the flight audio stream is playing");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resource_carries_physical_quantities_across() {
        let sound = AircraftSound {
            throttle: 0.6,
            airspeed: MetersPerSecond(45.0),
            stall_warning: true,
            muted: false,
        };
        let converted = sound.to_flight_sound();
        assert!((converted.throttle - 0.6).abs() < 1e-12);
        assert!((converted.airspeed - 45.0).abs() < 1e-12);
        assert!(converted.stall_warning);
        assert!(!converted.muted);
    }

    #[test]
    fn nothing_sounds_before_the_app_says_anything() {
        // **起動音を鳴らさない。** 既定は全閉・静止・警報なし。
        let sound = AircraftSound::default();
        assert!(sound.throttle.abs() < f64::EPSILON);
        assert!(sound.airspeed.get().abs() < f64::EPSILON);
        assert!(!sound.stall_warning);
    }

    #[test]
    fn the_default_volume_is_not_loud_enough_to_startle() {
        // 起動していきなり大音量は事故。下げる手段を探す前に切られる。
        assert!(AudioSettings::default().master <= 0.5);
        assert!(AudioSettings::default().enabled);
    }
}
