//! リプレイファイルを GUI 無しで 1 本作る。
//!
//! ```text
//! cargo run -p flightsim-sim --example record_takeoff -- flight-001.fsreplay
//! cargo run -p flightsim-app --release -- --replay flight-001.fsreplay
//! ```
//!
//! **人が飛ばさずに再生経路を試せるようにするためのもの。** 表示や操作を
//! 確認するたびに手で離陸していると、確認そのものが面倒になって省かれる。
//!
//! フライトディレクタが操縦するので、同じ引数なら毎回同じ記録になる。

use std::path::PathBuf;
use std::process::ExitCode;

use flightsim_core::{Geodetic, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_sim::replay::Conditions;
use flightsim_sim::{
    DirectorTargets, FlightDirector, GroundSampler, Recorder, Simulation, VerticalTarget,
};
use flightsim_world::{MemoryTileSource, Terrain};

/// 記録するフレーム数。60 fps 換算で 60 秒。
const FRAMES: u32 = 3_600;

/// 1 フレームの時間。**一定で回す。** ここは実行の再現性が要るところで、
/// 描画フレームのばらつきを真似る意味はない。
const FRAME_TIME: Seconds = Seconds(1.0 / 60.0);

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("flight-001.fsreplay"), PathBuf::from);

    let start = Geodetic::from_degrees(35.55, 139.78, 0.0);
    let heading = Radians::ZERO;
    let config = AircraftConfig::light_single();

    let mut simulation = Simulation::parked(
        config.clone(),
        start,
        heading,
        Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    let mut recorder = Recorder::new(
        Conditions {
            start,
            heading,
            ..Conditions::default()
        }
        .with_aircraft(&config),
    );

    let director = FlightDirector::default();
    for _ in 0..FRAMES {
        let agl = simulation.agl();
        // 離陸滑走のあいだは翼端を擦らないよう水平を保ち、浮いたら
        // 場周高度へ上げる。
        let rolling = agl < Meters(15.0);
        let controls = director.control(
            simulation.state(),
            agl,
            DirectorTargets {
                vertical: VerticalTarget::AltitudeAgl(Meters(300.0)),
                heading,
                airspeed: MetersPerSecond(45.0),
                flaps: 0.0,
                brakes: 0.0,
                throttle_override: Some(1.0),
                wings_level: rolling,
            },
        );
        recorder.record(FRAME_TIME, controls, Some(simulation.state()));
        let report = simulation.advance(FRAME_TIME, controls);
        if report.diverged {
            eprintln!("error: the flight diverged; not writing a recording of nonsense");
            return ExitCode::FAILURE;
        }
    }

    let recording = recorder.finish();
    let file = match std::fs::File::create(&path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("error: could not create {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let mut writer = std::io::BufWriter::new(file);
    if let Err(error) = recording.write_to(&mut writer) {
        eprintln!("error: could not write {}: {error}", path.display());
        return ExitCode::FAILURE;
    }
    // BufWriter は drop 時の失敗を握り潰す。ここを省くと「書けた」と
    // 言った直後に中身が欠ける。
    if let Err(error) = std::io::Write::flush(&mut writer) {
        eprintln!(
            "error: could not finish writing {}: {error}",
            path.display()
        );
        return ExitCode::FAILURE;
    }

    // 何が記録されたかを数字で出す。「書けた」だけでは中身が分からない。
    let ended = simulation.state().geodetic();
    eprintln!(
        "wrote {} frames ({:.0} s, {} keyframes) to {}",
        recording.frames().len(),
        recording.duration().get(),
        recording.keyframes().len(),
        path.display()
    );
    eprintln!(
        "ended at {:.5}, {:.5}  {:.0} m AGL  heading {:.0} deg",
        ended.latitude_degrees(),
        ended.longitude_degrees(),
        simulation.agl().get(),
        simulation.state().attitude().yaw.to_degrees().get(),
    );
    ExitCode::SUCCESS
}
