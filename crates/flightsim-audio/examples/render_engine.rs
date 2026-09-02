//! 飛行の音を WAV に書き出して、耳と目で確かめる。
//!
//! ```text
//! cargo run -p flightsim-audio --example render_engine -- flight.wav
//! ```
//!
//! **音は聞かないと分からない。** 検査は「狙った周波数に山があるか」までしか
//! 見られないので、実際の音を出す手段を用意しておく。GUI を起動せずに
//! 音だけを聞き比べられるので、調整のたびに飛ぶ必要がない。
//!
//! 書き出すのは 1 回の飛行を模した 20 秒:
//!
//! | 時刻 | 何が起きるか | 何を聞くか |
//! |---|---|---|
//! | 0〜2 s | アイドル | 全閉でも鳴っていること |
//! | 2〜6 s | 離陸滑走 → 上昇 | 出力で高さと**音色**が変わること |
//! | 6〜12 s | 巡航 | 速度で風切り音が明るくなること |
//! | 12〜16 s | 減速して失速警報 | 警報がエンジン音に埋もれないこと |
//! | 16〜20 s | 出力を戻す | 戻りが滑らかで、段差が出ないこと |

// 標本数と時刻の変換。48 kHz × 20 秒（96 万）は f64 の仮数にも usize にも
// 収まる。**書き出しの道具なので、ここで精度を気にする意味は無い。**
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "48 kHz × 20 秒の標本数は f64 にも usize にも収まる"
)]

use std::path::PathBuf;
use std::process::ExitCode;

use flightsim_audio::dsp::SAMPLE_RATE;
use flightsim_audio::engine::{EngineSpec, estimate_rpm};
use flightsim_audio::mixer::{DEFAULT_MASTER, FlightSound, Mixer};

/// 書き出す長さ `秒`。
const SECONDS: f64 = 20.0;

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("engine.wav"), PathBuf::from);

    let spec = EngineSpec::default();
    let mut mixer = Mixer::new(spec, DEFAULT_MASTER);
    let count = (SAMPLE_RATE * SECONDS) as usize;
    let mut samples = Vec::with_capacity(count);

    for index in 0..count {
        let seconds = index as f64 / SAMPLE_RATE;
        let input = flight_at(seconds);
        if index == 0 {
            mixer.reset(input);
        }
        samples.push(mixer.tick(input, DEFAULT_MASTER));
    }

    let peak = samples.iter().fold(0.0_f64, |peak, s| peak.max(s.abs()));
    let wav = to_wav(&samples);
    if let Err(error) = std::fs::write(&path, &wav) {
        eprintln!("error: could not write {}: {error}", path.display());
        return ExitCode::FAILURE;
    }

    // 何を書いたかを数字で出す。「書けた」だけでは中身が分からない。
    eprintln!(
        "wrote {:.0} s to {} ({} bytes, peak {peak:.3})",
        SECONDS,
        path.display(),
        wav.len()
    );
    eprintln!(
        "idle {:.0} rpm -> firing {:.1} Hz;  full {:.0} rpm -> firing {:.1} Hz",
        spec.idle_rpm,
        spec.firing_hz(spec.idle_rpm),
        spec.max_rpm,
        spec.firing_hz(spec.max_rpm),
    );
    eprintln!(
        "cruise: {:.0} rpm at full throttle and 55 m/s",
        estimate_rpm(spec, 1.0, 55.0)
    );
    ExitCode::SUCCESS
}

/// この時刻に機体がどうなっているか。
///
/// **1 回の飛行を模す。** 定常の音だけ聞いても、変化のなめらかさや
/// 警報の通りやすさは分からない。
fn flight_at(seconds: f64) -> FlightSound {
    // (時刻, 出力, 対気速度, 警報)
    let key_frames = [
        (0.0, 0.0, 0.0, false),
        (2.0, 0.0, 0.0, false),
        (4.0, 1.0, 30.0, false),
        (6.0, 1.0, 50.0, false),
        (12.0, 0.75, 62.0, false),
        (14.0, 0.15, 30.0, false),
        (16.0, 0.15, 24.0, true),
        (18.0, 0.9, 45.0, false),
        (SECONDS, 0.9, 50.0, false),
    ];

    let mut previous = key_frames[0];
    for frame in key_frames {
        if seconds <= frame.0 {
            let span = (frame.0 - previous.0).max(1e-9);
            let position = ((seconds - previous.0) / span).clamp(0.0, 1.0);
            return FlightSound {
                throttle: previous.1 + (frame.1 - previous.1) * position,
                airspeed: previous.2 + (frame.2 - previous.2) * position,
                // 警報は途中で入り切りする。**混ぜない。**
                stall_warning: frame.3 && position > 0.5,
                muted: false,
            };
        }
        previous = frame;
    }
    FlightSound {
        throttle: previous.1,
        airspeed: previous.2,
        stall_warning: false,
        muted: false,
    }
}

/// 16 bit PCM モノラルの WAV にする。
fn to_wav(samples: &[f64]) -> Vec<u8> {
    let rate = SAMPLE_RATE as u32;
    let data_bytes = u32::try_from(samples.len() * 2).expect("the sample count fits in a u32");
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        let scaled = (sample * 0.85 * 32_767.0).clamp(-32_767.0, 32_767.0);
        let value = scaled as i16;
        wav.extend_from_slice(&value.to_le_bytes());
    }
    wav
}
