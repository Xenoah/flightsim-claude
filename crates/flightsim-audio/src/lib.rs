//! # flightsim-audio
//!
//! エンジン音・風切り音・失速警報。**波形は同梱ファイルではなくコードで作る**
//! （[`synth`]、[ADR-0009]）。
//!
//! ## なぜ音が要るのか
//!
//! 失速が近いことを知る手段が、これが無いと**計器を見ることしかない**。
//! 着陸の最終段階で計器を見ている余裕は無いので、実機には必ず失速警報が付いて
//! いる。エンジン音と風切り音も同じで、出力と速度を耳で追えると、視線を外に
//! 置いたまま機体の状態が分かる。
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
//! ## 鳴らし方
//!
//! 3 つの波形をそれぞれループ再生し、音量と再生速度を毎フレーム変える。
//! **鳴らし直さない。** 都度 spawn すると、フレームごとに頭切れの音が
//! 重なって「ブツブツ」になる。
//!
//! [ADR-0009]: https://github.com/Xenoah/flightsim-claude/blob/main/docs/adr/0009-synthesised-audio.md

#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy の system は Res<T> / Query<T> を値で受け取るのが必須のイディオム。参照に変えると system として登録できない"
)]

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use flightsim_core::MetersPerSecond;

pub mod synth;

/// エンジン音の基準となる羽根通過周波数 `Hz`。
///
/// 2 枚羽根・2400 rpm 相当（2400/60 × 2 = 80）。**再生速度で上下させる**ので、
/// ここは基準の 1 点。
pub const ENGINE_BLADE_PASSAGE_HZ: f64 = 80.0;

/// 失速警報の高さ `Hz`。
///
/// 実機のリード式ホーンはおおむね数百 Hz から 2 kHz。エンジン音（基音 80 Hz と
/// その倍音）と離しつつ、耳障りすぎない高さとして 800 Hz を採る。
pub const STALL_HORN_HZ: f64 = 800.0;

/// 風切り音の素材の長さ `秒`。
///
/// 長いほど繰り返し感が薄れるが、そのぶん容量を食う（1 秒あたり約 88 kB）。
pub const WIND_SECONDS: f64 = 2.0;

/// 風切り音が最大音量になる対気速度。
///
/// この機体の超過禁止速度はモデルに無いので、**巡航より十分速い**ところに置く。
/// 63 m/s ≒ 122 kt。
const WIND_FULL_SCALE: f64 = 63.0;

/// 風切り音が鳴り始める対気速度。これ以下は無音。
const WIND_THRESHOLD: f64 = 10.0;

/// 機体が今どういう音を出しているか。app が毎フレーム埋める。
///
/// **物理量で渡す。** ここで音量や再生速度を受け取る形にすると、
/// 「どういう音にするか」の判断が app に散る。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct AircraftSound {
    /// 出力 `[0, 1]`。エンジン音の高さと大きさを決める。
    pub throttle: f64,
    /// 対気速度。風切り音の大きさを決める。
    pub airspeed: MetersPerSecond,
    /// 失速警報を鳴らすか。
    pub stall_warning: bool,
    /// 音を止めるか。一時停止・墜落・再生前に立てる。
    pub muted: bool,
}

/// どの音源かの印。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundChannel {
    /// エンジン。
    Engine,
    /// 風切り。
    Wind,
    /// 失速警報。
    StallWarning,
}

/// 音の設定。
///
/// 全体の音量を落としたり、丸ごと黙らせたりするのに使う。
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
            // 控えめに始める。**起動していきなり大音量は事故**で、
            // 下げる手段を探す前にスピーカーを切られる。
            master: 0.5,
            enabled: true,
        }
    }
}

/// 音を鳴らすプラグイン。
#[derive(Debug, Default)]
pub struct FlightAudioPlugin;

impl Plugin for FlightAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AircraftSound>()
            .init_resource::<AudioSettings>()
            .add_systems(Startup, spawn_sound_sources)
            .add_systems(Update, (update_sound_sources, report_playback));
    }
}

/// 波形を作って、ループ再生する音源を 3 つ置く。
///
/// **ここで作った音源を鳴らし続ける。** 都度 spawn しないのは、頭切れの音が
/// 重なって「ブツブツ」になるため。
pub fn spawn_sound_sources(
    mut commands: Commands,
    settings: Res<AudioSettings>,
    mut sources: ResMut<Assets<AudioSource>>,
) {
    if !settings.enabled {
        info!("audio: disabled");
        return;
    }

    for (channel, samples) in [
        (SoundChannel::Engine, synth::engine(ENGINE_BLADE_PASSAGE_HZ)),
        (SoundChannel::Wind, synth::wind(WIND_SECONDS)),
        (SoundChannel::StallWarning, synth::stall_horn(STALL_HORN_HZ)),
    ] {
        let wav = synth::to_wav(&samples);
        // **鳴らない不具合は静かに起きる。** WAV が壊れていても
        // rodio はエラーを出さず、ただ無音になる。作った物の大きさを
        // 出しておくと、少なくとも「生成はできている」ことが分かる。
        info!(
            "audio: {} = {} samples ({:.1} kB)",
            match channel {
                SoundChannel::Engine => "engine",
                SoundChannel::Wind => "wind",
                SoundChannel::StallWarning => "stall warning",
            },
            samples.len(),
            wav.len() as f64 / 1024.0
        );
        let source = sources.add(AudioSource { bytes: wav.into() });
        commands.spawn((
            AudioPlayer(source),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                // 無音で始める。**起動音を鳴らさない。**
                volume: Volume::Linear(0.0),
                ..PlaybackSettings::LOOP
            },
            channel,
            Name::new(match channel {
                SoundChannel::Engine => "engine sound",
                SoundChannel::Wind => "wind sound",
                SoundChannel::StallWarning => "stall warning",
            }),
        ));
    }
}

/// 再生が始まったことを一度だけ出す。
///
/// **`AudioSink` は再生が始まって初めて付く。** これが出ないなら、
/// 音源は作れたが鳴っていない（出力機器が無い、WAV が読めない、など）。
/// 無音の原因を「合成が悪いのか、再生に届いていないのか」に切り分けられる。
pub fn report_playback(sinks: Query<&SoundChannel, Added<AudioSink>>) {
    for channel in &sinks {
        info!("audio: {channel:?} is playing");
    }
}

/// 機体の状態に合わせて音量と再生速度を変える。
pub fn update_sound_sources(
    sound: Res<AircraftSound>,
    settings: Res<AudioSettings>,
    mut sinks: Query<(&SoundChannel, &mut AudioSink)>,
) {
    for (channel, mut sink) in &mut sinks {
        let level = mix(*channel, &sound, &settings);
        sink.set_volume(Volume::Linear(level.volume));
        sink.set_speed(level.speed);
    }
}

/// 1 つの音源に与える音量と再生速度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundLevel {
    /// 線形音量。
    pub volume: f32,
    /// 再生速度。1.0 が元の高さ。
    pub speed: f32,
}

/// 状態から音量と再生速度を決める。
///
/// **Bevy に触らない純関数。** 音の決め方はここだけを読めば分かる。
#[must_use]
pub fn mix(channel: SoundChannel, sound: &AircraftSound, settings: &AudioSettings) -> SoundLevel {
    let master = clamp_unit(settings.master);
    if sound.muted || !settings.enabled {
        // 止めるときも再生は続ける。**止め方を「音源を消す」にすると、
        // 再開のたびに作り直すことになり頭が切れる。**
        return SoundLevel {
            volume: 0.0,
            speed: 1.0,
        };
    }

    match channel {
        SoundChannel::Engine => {
            let throttle = clamp_unit(sound.throttle);
            // アイドルでも回っている。**全閉で無音になると、
            // エンストしたのか音が壊れたのか分からない。**
            let volume = throttle.mul_add(0.75, 0.25) * master * 0.55;
            // 全閉で 0.7 倍、全開で 1.35 倍。回転数の幅としては控えめだが、
            // 上げすぎると合成音が甲高くなって耳につく。
            let speed = throttle.mul_add(0.65, 0.7);
            SoundLevel {
                volume: to_f32(volume),
                speed: to_f32(speed),
            }
        }
        SoundChannel::Wind => {
            let speed = sound.airspeed.get();
            let volume = if !speed.is_finite() || speed <= WIND_THRESHOLD {
                0.0
            } else {
                let position =
                    ((speed - WIND_THRESHOLD) / (WIND_FULL_SCALE - WIND_THRESHOLD)).clamp(0.0, 1.0);
                // 二乗で効かせる。**線形だと低速から鳴りすぎて、
                // 速度が上がった実感が出ない。**
                position * position * master * 0.5
            };
            SoundLevel {
                volume: to_f32(volume),
                speed: 1.0,
            }
        }
        SoundChannel::StallWarning => SoundLevel {
            // 警報はほかの音に埋もれてはいけない。**大きめに出す。**
            volume: to_f32(if sound.stall_warning {
                master * 0.6
            } else {
                0.0
            }),
            speed: 1.0,
        },
    }
}

/// `[0, 1]` に収める。NaN は 0。
fn clamp_unit(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// 音量・速度を `f32` にする。**非有限は無音側へ倒す。**
fn to_f32(value: f64) -> f32 {
    if value.is_finite() {
        // 音量と速度はどちらも 0〜数倍の範囲。f32 の精度で足りる。
        #[expect(clippy::cast_possible_truncation, reason = "音量と速度は小さな有限値")]
        let value = value as f32;
        value
    } else {
        0.0
    }
}

#[cfg(test)]
// 音量の 0 と頭打ちは「ちょうどこの値」が契約なので、厳密比較で検査する。
// 近似で見ると、無音のはずが極小の音で鳴っていても通ってしまう。
#[expect(clippy::float_cmp, reason = "無音と頭打ちは厳密な値が契約")]
mod tests {
    use super::*;

    fn settings() -> AudioSettings {
        AudioSettings::default()
    }

    fn flying(throttle: f64, airspeed: f64) -> AircraftSound {
        AircraftSound {
            throttle,
            airspeed: MetersPerSecond(airspeed),
            stall_warning: false,
            muted: false,
        }
    }

    #[test]
    fn opening_the_throttle_raises_the_engine_note() {
        // **これが逆だと、音が状態を誤って伝える。**
        let idle = mix(SoundChannel::Engine, &flying(0.0, 0.0), &settings());
        let full = mix(SoundChannel::Engine, &flying(1.0, 0.0), &settings());
        assert!(full.speed > idle.speed, "{full:?} vs {idle:?}");
        assert!(full.volume > idle.volume);
    }

    #[test]
    fn the_engine_is_audible_even_at_idle() {
        // **全閉で無音になると、エンストしたのか音が壊れたのか分からない。**
        let idle = mix(SoundChannel::Engine, &flying(0.0, 0.0), &settings());
        assert!(idle.volume > 0.0);
        assert!(idle.speed > 0.0, "a speed of 0 would stop the sound dead");
    }

    #[test]
    fn the_wind_is_silent_when_parked_and_loud_when_fast() {
        let parked = mix(SoundChannel::Wind, &flying(0.0, 0.0), &settings());
        assert_eq!(parked.volume, 0.0, "a parked aircraft has no wind noise");

        let cruise = mix(SoundChannel::Wind, &flying(0.5, 50.0), &settings());
        let dive = mix(SoundChannel::Wind, &flying(0.5, 80.0), &settings());
        assert!(cruise.volume > 0.0);
        assert!(dive.volume > cruise.volume, "faster must be louder");
    }

    #[test]
    fn the_wind_does_not_get_louder_beyond_full_scale() {
        // 頭打ちが無いと、速度超過で音量が 1 を超えて割れる。
        let fast = mix(SoundChannel::Wind, &flying(0.5, 200.0), &settings());
        let faster = mix(SoundChannel::Wind, &flying(0.5, 2_000.0), &settings());
        assert_eq!(fast.volume, faster.volume);
        assert!(fast.volume <= 1.0);
    }

    #[test]
    fn the_stall_warning_is_only_on_when_asked() {
        let quiet = mix(SoundChannel::StallWarning, &flying(0.5, 40.0), &settings());
        assert_eq!(quiet.volume, 0.0);

        let mut warned = flying(0.5, 40.0);
        warned.stall_warning = true;
        let loud = mix(SoundChannel::StallWarning, &warned, &settings());
        assert!(loud.volume > 0.0);
    }

    #[test]
    fn the_stall_warning_is_not_drowned_out_by_the_engine() {
        // **警報が聞こえなければ意味がない。**
        let mut warned = flying(1.0, 40.0);
        warned.stall_warning = true;
        let horn = mix(SoundChannel::StallWarning, &warned, &settings());
        let engine = mix(SoundChannel::Engine, &warned, &settings());
        assert!(
            horn.volume >= engine.volume,
            "the horn ({}) must not be quieter than the engine at full power ({})",
            horn.volume,
            engine.volume
        );
    }

    #[test]
    fn muting_silences_everything_but_keeps_the_sources_playing() {
        // 止め方を「音源を消す」にすると、再開のたびに作り直して頭が切れる。
        let mut muted = flying(1.0, 60.0);
        muted.stall_warning = true;
        muted.muted = true;
        for channel in [
            SoundChannel::Engine,
            SoundChannel::Wind,
            SoundChannel::StallWarning,
        ] {
            let level = mix(channel, &muted, &settings());
            assert_eq!(level.volume, 0.0, "{channel:?} should be silent");
            assert!(level.speed > 0.0, "{channel:?} must keep playing silently");
        }
    }

    #[test]
    fn the_master_volume_scales_every_channel() {
        let mut loud = flying(1.0, 60.0);
        loud.stall_warning = true;
        let half = AudioSettings {
            master: 0.5,
            enabled: true,
        };
        let quarter = AudioSettings {
            master: 0.25,
            enabled: true,
        };
        for channel in [
            SoundChannel::Engine,
            SoundChannel::Wind,
            SoundChannel::StallWarning,
        ] {
            let a = mix(channel, &loud, &half).volume;
            let b = mix(channel, &loud, &quarter).volume;
            assert!(b < a, "{channel:?}: {b} should be quieter than {a}");
        }
    }

    #[test]
    fn broken_inputs_do_not_reach_the_speakers() {
        // **NaN の音量や速度を渡すと、鳴り止まない雑音になることがある。**
        let broken = AircraftSound {
            throttle: f64::NAN,
            airspeed: MetersPerSecond(f64::NAN),
            stall_warning: true,
            muted: false,
        };
        for channel in [
            SoundChannel::Engine,
            SoundChannel::Wind,
            SoundChannel::StallWarning,
        ] {
            let level = mix(channel, &broken, &settings());
            assert!(level.volume.is_finite(), "{channel:?} volume is not finite");
            assert!(level.speed.is_finite(), "{channel:?} speed is not finite");
            assert!((0.0..=1.0).contains(&level.volume));
            assert!(level.speed > 0.0);
        }
    }

    #[test]
    fn nothing_plays_when_audio_is_disabled() {
        let off = AudioSettings {
            master: 1.0,
            enabled: false,
        };
        let mut loud = flying(1.0, 60.0);
        loud.stall_warning = true;
        for channel in [
            SoundChannel::Engine,
            SoundChannel::Wind,
            SoundChannel::StallWarning,
        ] {
            assert_eq!(mix(channel, &loud, &off).volume, 0.0);
        }
    }

    #[test]
    fn the_default_is_not_loud_enough_to_startle() {
        // 起動していきなり大音量は事故。下げる手段を探す前に切られる。
        assert!(settings().master <= 0.5);
        let idle = mix(SoundChannel::Engine, &flying(0.0, 0.0), &settings());
        assert!(
            idle.volume < 0.3,
            "startup volume {} is too loud",
            idle.volume
        );
    }
}
