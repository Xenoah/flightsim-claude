//! 乱流の検査。
//!
//! **決定論が最優先**（ADR-0004）。乱数を使っていないことを、
//! 出力のビット一致で確かめる。

use flightsim_core::{Geodetic, MetersPerSecond, Seconds};
use flightsim_fdm::Turbulence;

fn somewhere() -> Geodetic {
    Geodetic::from_degrees(35.55, 139.78, 500.0)
}

fn moderate() -> Turbulence {
    Turbulence::moderate(12_345)
}

#[test]
fn the_same_input_gives_bit_identical_output() {
    // リプレイとネットワーク同期の前提。**近い値ではなく同じ値**であること。
    let turbulence = moderate();
    for step in 0..2000 {
        let time = Seconds(f64::from(step) / 120.0);
        let first = turbulence.gust_at(time, somewhere());
        let second = turbulence.gust_at(time, somewhere());
        assert_eq!(
            first.north().to_bits(),
            second.north().to_bits(),
            "the same input produced different bits at t={time}"
        );
        assert_eq!(first.east().to_bits(), second.east().to_bits());
        assert_eq!(first.down().to_bits(), second.down().to_bits());
    }
}

#[test]
fn a_different_seed_gives_a_different_atmosphere() {
    // 種が効いていないと、毎回同じ揺れ方になる。
    let a = Turbulence::moderate(1).gust_at(Seconds(10.0), somewhere());
    let b = Turbulence::moderate(2).gust_at(Seconds(10.0), somewhere());
    let difference = (a.north() - b.north()).abs() + (a.east() - b.east()).abs();
    assert!(
        difference > 0.01,
        "two seeds produced nearly the same gust: {difference}"
    );
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "無乱流は近似ゼロではなく**厳密にゼロ**であること自体が要件。許容誤差を入れると『ほぼ無風』を見逃す"
)]
fn calm_air_is_exactly_still() {
    for step in 0..500 {
        let gust = Turbulence::CALM.gust_at(Seconds(f64::from(step) * 0.1), somewhere());
        assert_eq!(gust.north(), 0.0);
        assert_eq!(gust.east(), 0.0);
        assert_eq!(gust.down(), 0.0);
    }
}

#[test]
fn the_gust_does_not_jump_between_physics_steps() {
    // **1 ステップごとに独立な乱数だと機体が痙攣する。**
    // 120 Hz の刻みで擾乱が飛ばないこと。
    let turbulence = Turbulence::severe(7);
    let mut previous = turbulence.gust_at(Seconds(0.0), somewhere());
    let mut worst: f64 = 0.0;

    for step in 1..2400 {
        let gust = turbulence.gust_at(Seconds(f64::from(step) / 120.0), somewhere());
        let change = (gust.north() - previous.north())
            .abs()
            .max((gust.east() - previous.east()).abs())
            .max((gust.down() - previous.down()).abs());
        worst = worst.max(change);
        previous = gust;
    }

    // 時定数 4 秒・強度 6 m/s なら、1/120 秒あたりの変化は
    // せいぜい数 cm/s のはず。0.2 m/s を超えたら滑らかとは言えない。
    assert!(
        worst < 0.2,
        "the gust jumped by {worst:.3} m/s within one physics step"
    );
}

#[test]
fn the_field_is_smooth_in_space_too() {
    // 位置がわずかに動いただけで擾乱が飛ばないこと。
    // 巡航 50 m/s なら 1 ステップで 0.42 m 進む。
    let turbulence = Turbulence::severe(7);
    let base = somewhere();
    let mut worst: f64 = 0.0;

    for step in 0..2000 {
        // 北へ 0.42 m ずつ。
        let metres = f64::from(step) * 0.42;
        let moved = base.offset_by(flightsim_core::Meters(metres), flightsim_core::Meters(0.0));
        let next = base.offset_by(
            flightsim_core::Meters(metres + 0.42),
            flightsim_core::Meters(0.0),
        );
        let a = turbulence.gust_at(Seconds(0.0), moved);
        let b = turbulence.gust_at(Seconds(0.0), next);
        worst = worst.max(
            (a.north() - b.north())
                .abs()
                .max((a.down() - b.down()).abs()),
        );
    }

    assert!(
        worst < 0.2,
        "the gust jumped by {worst:.3} m/s over 0.42 m of travel"
    );
}

#[test]
fn the_gust_stays_within_the_requested_intensity() {
    // 有界であること。強度の数倍を超える突風が出ると、機体が吹き飛ぶ。
    for turbulence in [
        Turbulence::light(1),
        Turbulence::moderate(2),
        Turbulence::severe(3),
    ] {
        let limit = turbulence.intensity.get();
        for step in 0..5000 {
            let time = Seconds(f64::from(step) * 0.05);
            let gust = turbulence.gust_at(time, somewhere());
            for (name, value) in [
                ("north", gust.north()),
                ("east", gust.east()),
                ("down", gust.down()),
            ] {
                assert!(
                    value.abs() <= limit * 1.01,
                    "{name} gust {value} exceeded the intensity {limit}"
                );
            }
        }
    }
}

#[test]
fn the_typical_gust_is_a_meaningful_fraction_of_the_intensity() {
    // 有界なだけでなく、実際に揺れていること。
    // 値ノイズの RMS は振幅の 0.2〜0.5 倍程度になる。ここは
    // 「強度に見合う揺れがある」ことだけを確かめる。
    let turbulence = Turbulence::moderate(99);
    let mut sum_squares = 0.0;
    let samples = 4000;
    for step in 0..samples {
        let gust = turbulence.gust_at(Seconds(f64::from(step) * 0.05), somewhere());
        sum_squares += gust.down() * gust.down();
    }
    let rms = (sum_squares / f64::from(samples)).sqrt();
    let intensity = turbulence.intensity.get();
    assert!(
        rms > intensity * 0.05 && rms < intensity,
        "the vertical RMS {rms:.3} is not a sensible fraction of {intensity}"
    );
}

#[test]
fn broken_inputs_do_not_poison_the_air() {
    // **NaN を大気へ入れると全状態へ伝播する。**
    let turbulence = Turbulence::severe(5);
    let broken_places = [
        Geodetic::from_degrees(f64::NAN, 139.0, 100.0),
        Geodetic::from_degrees(35.0, f64::INFINITY, 100.0),
        Geodetic::from_degrees(35.0, 139.0, f64::NAN),
    ];
    for place in broken_places {
        let gust = turbulence.gust_at(Seconds(1.0), place);
        assert!(gust.north().is_finite() && gust.east().is_finite() && gust.down().is_finite());
    }
    for time in [f64::NAN, f64::INFINITY] {
        let gust = turbulence.gust_at(Seconds(time), somewhere());
        assert!(gust.north().is_finite() && gust.down().is_finite());
    }

    // 強度そのものが壊れていても無風へ倒れる。
    let broken = Turbulence {
        intensity: MetersPerSecond(f64::NAN),
        seed: 1,
    };
    let gust = broken.gust_at(Seconds(1.0), somewhere());
    assert!(
        gust.north() == 0.0 && gust.east() == 0.0 && gust.down() == 0.0,
        "a broken intensity must fall back to exactly still air, got {gust:?}"
    );
}

#[test]
fn a_still_aircraft_still_feels_the_air_move() {
    // 空間相関だけだと、止まっている機体がまったく揺れない。
    // 時間相関が効いていることを確かめる。
    let turbulence = Turbulence::moderate(11);
    let first = turbulence.gust_at(Seconds(0.0), somewhere());
    let later = turbulence.gust_at(Seconds(8.0), somewhere());
    let change = (first.down() - later.down()).abs();
    assert!(
        change > 0.05,
        "a parked aircraft never felt the air change: {change}"
    );
}
