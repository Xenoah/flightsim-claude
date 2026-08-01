//! 空力の内訳を時系列で出力する診断ツール。
//!
//! 空力係数を調整する際、テストの合否だけでは「なぜその値になったか」が分からない。
//! ロール率・迎角・横滑り角・各係数を並べて見ることで、どの項が効いているかを特定できる。
//!
//! ```text
//! cargo run -p flightsim-fdm --example aero_trace
//! ```
//!
//! 既定では全横操舵に対するロール応答を追う。別の操縦入力を調べたい場合は
//! [`controls`] を書き換えること。

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "ステップ数の算出。秒数も刻み幅もこのファイル内の既知の正の定数"
)]

use flightsim_core::{Attitude, Geodetic, Ned};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, Environment, FlightDynamics, RECOMMENDED_FIXED_DT,
    RigidBodyState, aero,
};

/// 追跡する操縦入力。
fn controls() -> ControlInputs {
    ControlInputs::neutral()
        .with_throttle(0.7)
        .with_aileron(1.0)
}

/// 追跡する秒数。
const DURATION_SECONDS: f64 = 3.0;

/// 出力間隔（ステップ数）。
const SAMPLE_INTERVAL: u32 = 30;

fn main() {
    let config = AircraftConfig::light_single();

    println!("aircraft: {}", config.name);
    println!(
        "  roll:  C_lδa={:+.5}  C_lp={:+.4}  C_lβ={:+.4}",
        config.aero.roll_aileron, config.aero.roll_rate_p, config.aero.roll_beta
    );
    println!(
        "  yaw:   C_nδa={:+.5}  C_nr={:+.4}  C_nβ={:+.4}",
        config.aero.yaw_aileron, config.aero.yaw_rate_r, config.aero.yaw_beta
    );
    println!(
        "  mass:  {:.0} kg   Ixx={:.0}  Iyy={:.0}  Izz={:.0}",
        config.mass_properties.mass().get(),
        config.mass_properties.inertia().x_axis.x,
        config.mass_properties.inertia().y_axis.y,
        config.mass_properties.inertia().z_axis.z,
    );
    println!();
    println!(
        "{:>6}  {:>9}  {:>7}  {:>7}  {:>7}  {:>9}  {:>9}  {:>7}",
        "t [s]", "p [°/s]", "V `m/s`", "α [°]", "β [°]", "C_l", "C_m", "bank [°]"
    );

    let state = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.0, 139.0, 2_000.0),
        Attitude::default(),
        Ned::new(55.0, 0.0, 0.0),
    );
    let mut fdm = FlightDynamics::new(config, state);
    let environment = Environment::still_air();
    let controls = controls();

    let steps = (DURATION_SECONDS / RECOMMENDED_FIXED_DT.get()).round() as u32;

    for step in 0..=steps {
        if step % SAMPLE_INTERVAL == 0 {
            let state = fdm.state();
            let angles = aero::aero_angles(state.body_velocity());
            let coefficients = aero::coefficients(
                &fdm.config().aero,
                &fdm.config().geometry,
                angles,
                state.angular_velocity,
                controls,
            );

            println!(
                "{:>6.2}  {:>9.1}  {:>7.2}  {:>7.2}  {:>7.2}  {:>9.5}  {:>9.5}  {:>7.1}",
                f64::from(step) * RECOMMENDED_FIXED_DT.get(),
                state.angular_velocity.x.to_degrees(),
                angles.true_airspeed.get(),
                angles.angle_of_attack.to_degrees().get(),
                angles.sideslip.to_degrees().get(),
                coefficients.roll,
                coefficients.pitch,
                state.attitude().roll.to_degrees().get(),
            );
        }

        fdm.step(RECOMMENDED_FIXED_DT, controls, &environment);
    }
}
