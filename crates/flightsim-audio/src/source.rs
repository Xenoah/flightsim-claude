//! 生成した音を Bevy（rodio）へ流す。
//!
//! # なぜ WAV のループ再生をやめたのか
//!
//! 前の作りは、波形を 1 本作って**再生速度を変えてピッチを上げ下げ**して
//! いた。これには避けようのない欠点がある:
//!
//! - **音色まで一緒に伸び縮みする。** 回転を上げると全体が早回しになり、
//!   共鳴も倍音も揃って移調する。実機は共鳴が動かないので、これは
//!   「テープの早送り」に聞こえる
//! - 出力が変わったときにパルスの形を変えられない。実機は筒内圧が上がると
//!   排気の立ち上がりが鋭くなり、音が硬くなる
//! - 回転を連続的に動かすと、標本の読み位置が飛んで濁る
//!
//! そこで**毎標本その場で作る**形に変えた。回転数はパルスの間隔を変え、
//! 共鳴（フォルマント）はその場に留まる。
//!
//! # 音の処理は別の thread で走る
//!
//! rodio は自前の thread で標本を取りに来る。ECS からは値を渡すだけで、
//! **鍵（Mutex）は使わない。** 音の thread が鍵待ちで止まると、その瞬間
//! 出力が途切れて「プツッ」と鳴る。値の受け渡しは原子変数で行う。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use bevy::asset::Asset;
use bevy::audio::Decodable;
use bevy::reflect::TypePath;

use crate::dsp::SAMPLE_RATE;
use crate::mixer::{DEFAULT_MASTER, EngineKind, FlightSound, Mixer};

/// ECS と音の thread で共有する値。
///
/// **原子変数だけで持つ。** 音の thread で鍵を取ると、待たされた瞬間に
/// 出力が途切れる。`f32` は `to_bits` で `u32` として置く。
#[derive(Debug)]
pub struct SharedSound {
    throttle: AtomicU32,
    airspeed: AtomicU32,
    master: AtomicU32,
    stall_warning: AtomicBool,
    muted: AtomicBool,
    /// 立てると、音の thread が次の標本で状態を今の値へ飛ばす。
    /// やり直しの瞬間に使う。
    reset: AtomicBool,
}

impl Default for SharedSound {
    fn default() -> Self {
        Self {
            throttle: AtomicU32::new(0.0_f32.to_bits()),
            airspeed: AtomicU32::new(0.0_f32.to_bits()),
            master: AtomicU32::new((DEFAULT_MASTER as f32).to_bits()),
            stall_warning: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            reset: AtomicBool::new(false),
        }
    }
}

impl SharedSound {
    /// 機体の状態を書き込む。ECS 側から毎フレーム呼ぶ。
    pub fn set(&self, sound: FlightSound) {
        store(&self.throttle, sound.throttle);
        store(&self.airspeed, sound.airspeed);
        self.stall_warning
            .store(sound.stall_warning, Ordering::Relaxed);
        self.muted.store(sound.muted, Ordering::Relaxed);
    }

    /// 全体の音量を書き込む。
    pub fn set_master(&self, master: f64) {
        store(&self.master, master);
    }

    /// 次の標本で状態を飛ばすよう頼む。**やり直しの瞬間だけ。**
    pub fn request_reset(&self) {
        self.reset.store(true, Ordering::Relaxed);
    }

    /// 今の状態を読む。音の thread から呼ぶ。
    #[must_use]
    pub fn get(&self) -> FlightSound {
        FlightSound {
            throttle: load(&self.throttle),
            airspeed: load(&self.airspeed),
            stall_warning: self.stall_warning.load(Ordering::Relaxed),
            muted: self.muted.load(Ordering::Relaxed),
        }
    }

    /// 今の全体音量。
    #[must_use]
    pub fn master(&self) -> f64 {
        load(&self.master)
    }

    /// やり直しが頼まれていたら真を返し、印を降ろす。
    #[must_use]
    pub fn take_reset(&self) -> bool {
        self.reset.swap(false, Ordering::Relaxed)
    }
}

/// **`f32` へ落とす。** 音量・速度は f32 の精度で足りる。
fn store(slot: &AtomicU32, value: f64) {
    let value = if value.is_finite() { value as f32 } else { 0.0 };
    slot.store(value.to_bits(), Ordering::Relaxed);
}

fn load(slot: &AtomicU32) -> f64 {
    f64::from(f32::from_bits(slot.load(Ordering::Relaxed)))
}

/// Bevy の資産としての音源。
///
/// 中身は共有状態への参照だけ。**波形は持たない**（その場で作るので）。
#[derive(Asset, TypePath, Debug, Clone)]
pub struct FlightAudio {
    shared: Arc<SharedSound>,
    kind: EngineKind,
}

impl FlightAudio {
    /// 共有状態と機種から作る。
    #[must_use]
    pub const fn new(shared: Arc<SharedSound>, kind: EngineKind) -> Self {
        Self { shared, kind }
    }

    /// どの動力を鳴らすか。
    #[must_use]
    pub const fn kind(&self) -> EngineKind {
        self.kind
    }

    /// 共有状態。
    #[must_use]
    pub fn shared(&self) -> &Arc<SharedSound> {
        &self.shared
    }
}

impl Decodable for FlightAudio {
    type DecoderItem = f32;
    type Decoder = FlightAudioStream;

    fn decoder(&self) -> Self::Decoder {
        FlightAudioStream {
            mixer: Mixer::new(self.kind, DEFAULT_MASTER),
            shared: Arc::clone(&self.shared),
        }
    }
}

/// 標本を作り続ける流れ。**終わらない。**
///
/// rodio がこの iterator を回し、必要なだけ標本を引いていく。
#[derive(Debug)]
pub struct FlightAudioStream {
    mixer: Mixer,
    shared: Arc<SharedSound>,
}

impl Iterator for FlightAudioStream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let input = self.shared.get();
        if self.shared.take_reset() {
            self.mixer.reset(input);
        }
        let sample = self.mixer.tick(input, self.shared.master());
        // f32 へ落とす。**混ぜた時点で ±1 に収めてある。**
        let sample = sample as f32;
        Some(sample)
    }
}

impl rodio::Source for FlightAudioStream {
    fn current_frame_len(&self) -> Option<usize> {
        // 途切れない。標本化周波数もチャンネル数も変わらない。
        None
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE as u32
    }

    fn total_duration(&self) -> Option<Duration> {
        // 終わらない。
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source as _;

    fn shared() -> Arc<SharedSound> {
        Arc::new(SharedSound::default())
    }

    #[test]
    fn the_stream_never_ends() {
        // **終わる音源にすると、鳴り終わったあと二度と鳴らない。**
        let audio = FlightAudio::new(shared(), EngineKind::default());
        let stream = audio.decoder();
        assert_eq!(stream.total_duration(), None);
        assert_eq!(stream.current_frame_len(), None);
        assert_eq!(stream.channels(), 1);
        assert_eq!(stream.sample_rate(), 48_000);
    }

    #[test]
    fn the_stream_keeps_producing_samples() {
        let audio = FlightAudio::new(shared(), EngineKind::default());
        let mut stream = audio.decoder();
        for _ in 0..100_000 {
            let sample = stream.next().expect("the stream must not end");
            assert!(sample.is_finite());
            assert!((-1.0..=1.0).contains(&sample));
        }
    }

    #[test]
    fn what_the_ecs_writes_is_what_the_audio_thread_reads() {
        let shared = shared();
        shared.set(FlightSound {
            throttle: 0.75,
            airspeed: 42.0,
            stall_warning: true,
            muted: false,
        });
        let read = shared.get();
        assert!((read.throttle - 0.75).abs() < 1e-6);
        assert!((read.airspeed - 42.0).abs() < 1e-4);
        assert!(read.stall_warning);
        assert!(!read.muted);
    }

    #[test]
    fn broken_values_do_not_cross_the_boundary() {
        // **NaN が音の thread へ渡ると、鳴り止まない雑音になることがある。**
        let shared = shared();
        shared.set(FlightSound {
            throttle: f64::NAN,
            airspeed: f64::INFINITY,
            stall_warning: false,
            muted: false,
        });
        let read = shared.get();
        assert!(read.throttle.is_finite(), "{}", read.throttle);
        assert!(read.airspeed.is_finite(), "{}", read.airspeed);
    }

    #[test]
    fn a_reset_request_is_taken_once() {
        // 2 回効くと、やり直していないのに状態が飛ぶ。
        let shared = shared();
        assert!(!shared.take_reset());
        shared.request_reset();
        assert!(shared.take_reset());
        assert!(!shared.take_reset());
    }

    #[test]
    fn muting_through_the_shared_state_silences_the_stream() {
        let shared = shared();
        let audio = FlightAudio::new(Arc::clone(&shared), EngineKind::default());
        let mut stream = audio.decoder();
        shared.set(FlightSound {
            throttle: 1.0,
            airspeed: 60.0,
            stall_warning: true,
            muted: false,
        });
        let mut loud = 0.0_f32;
        for _ in 0..48_000 {
            loud = loud.max(stream.next().expect("still playing").abs());
        }
        assert!(loud > 0.01, "the stream was silent at {loud}");

        shared.set(FlightSound {
            throttle: 1.0,
            airspeed: 60.0,
            stall_warning: true,
            muted: true,
        });
        // 消音が効くまで少し流す。
        for _ in 0..24_000 {
            stream.next();
        }
        let mut quiet = 0.0_f32;
        for _ in 0..4_800 {
            quiet = quiet.max(stream.next().expect("still playing").abs());
        }
        assert!(quiet < 1e-3, "still audible at {quiet}");
    }

    #[test]
    fn the_master_volume_reaches_the_stream() {
        let shared = shared();
        let audio = FlightAudio::new(Arc::clone(&shared), EngineKind::default());
        let mut stream = audio.decoder();
        shared.set(FlightSound {
            throttle: 1.0,
            airspeed: 60.0,
            stall_warning: false,
            muted: false,
        });
        shared.set_master(0.0);
        for _ in 0..24_000 {
            stream.next();
        }
        let mut peak = 0.0_f32;
        for _ in 0..4_800 {
            peak = peak.max(stream.next().expect("still playing").abs());
        }
        assert!(peak < 1e-3, "master 0 still gave {peak}");
    }
}
