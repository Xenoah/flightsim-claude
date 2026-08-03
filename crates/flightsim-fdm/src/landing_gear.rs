//! 着陸装置の接地反力。
//!
//! 地形データはここで取得しない。呼び出し側が [`crate::Environment`] に渡した
//! ローカル接地平面だけを使い、`flightsim-world` への横断依存を避ける。

use flightsim_core::{Ecef, LocalFrame, Ned, Seconds};
use glam::DVec3;

use crate::{
    ControlInputs, Environment, RigidBodyState,
    aircraft::{LandingGearConfig, MassProperties},
};

/// 主ばねが最大ストロークに達した後のバンプストップ剛性倍率。
///
/// 主ばねを単に頭打ちにするとハードランディングで地面を抜ける。ストロークを超えた分だけ
/// 高剛性の二次ばねを働かせ、位置のクランプを使わずに底付きへ対処する。
const BOTTOM_OUT_STIFFNESS_MULTIPLIER: f64 = 6.0;

/// ばね固有振動の 1 サブステップあたり位相上限 `rad`。
pub(crate) const MAX_GEAR_PHASE_PER_SUBSTEP: f64 = 0.05;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GroundLoads {
    pub force_body: DVec3,
    pub moment_body: DVec3,
}

/// 接地平面の基準点からの指定オフセットにおける地面標高を返す。
fn ground_elevation_at(environment: &Environment, offset_ned: Ned) -> f64 {
    environment.ground_elevation.get()
        + environment.ground_slope().north() * offset_ned.north()
        + environment.ground_slope().east() * offset_ned.east()
}

fn ground_input_is_finite(environment: &Environment) -> bool {
    let reference_is_finite = environment.ground_reference().is_none_or(|reference| {
        reference.latitude.is_finite()
            && reference.longitude.is_finite()
            && reference.altitude.is_finite()
    });
    environment.ground_elevation.is_finite()
        && environment.ground_slope().is_finite()
        && reference_is_finite
}

fn ground_frame(environment: &Environment, current_frame: &LocalFrame) -> LocalFrame {
    environment
        .ground_reference()
        .map_or(*current_frame, LocalFrame::new)
}

/// 地面から上向きの単位法線を ECEF 系で返す。
fn ground_normal_ecef(environment: &Environment, frame: &LocalFrame) -> DVec3 {
    let normal_ned = DVec3::new(
        -environment.ground_slope().north(),
        -environment.ground_slope().east(),
        -1.0,
    )
    .normalize();
    frame.ned_to_ecef_vector(Ned(normal_ned))
}

/// 鉛直標高差を傾斜面への符号付き最短距離へ変換する係数。
fn vertical_to_normal_distance_scale(environment: &Environment) -> f64 {
    let north = environment.ground_slope().north();
    let east = environment.ground_slope().east();
    1.0 / (1.0 + north * north + east * east).sqrt()
}

/// 全脚の接地荷重を機体軸で合算する。
pub(crate) fn loads(
    gear: &LandingGearConfig,
    state: &RigidBodyState,
    controls: ControlInputs,
    environment: &Environment,
    frame: &LocalFrame,
) -> GroundLoads {
    if !ground_input_is_finite(environment) {
        return GroundLoads::default();
    }

    let ground_frame = ground_frame(environment, frame);
    let ground_normal = ground_normal_ecef(environment, &ground_frame);
    let normal_distance_scale = vertical_to_normal_distance_scale(environment);
    let forward_ecef = state.orientation * DVec3::X;
    let forward_tangent = forward_ecef - ground_normal * forward_ecef.dot(ground_normal);
    let has_tangent_basis = forward_tangent.length_squared() > 1.0e-12;
    let forward_tangent = if has_tangent_basis {
        forward_tangent.normalize()
    } else {
        DVec3::ZERO
    };
    let lateral_tangent = if has_tangent_basis {
        forward_tangent.cross(ground_normal).normalize()
    } else {
        DVec3::ZERO
    };

    let transition_speed = gear.friction_transition_speed().get();
    let forward_coefficient = gear.rolling_friction_coefficient()
        + controls.brakes() * gear.braking_friction_coefficient();
    let friction_limit = forward_coefficient.max(gear.lateral_friction_coefficient());

    let mut total = GroundLoads::default();
    for leg in gear.legs() {
        let contact_body = leg.contact_point().as_vec();
        let contact_offset_ecef = state.orientation * contact_body;
        let contact_ecef = Ecef::from_vec(state.position.as_vec() + contact_offset_ecef);
        let contact_offset_ned = ground_frame.ecef_to_ned_position(contact_ecef);
        let ground_elevation = ground_elevation_at(environment, contact_offset_ned);
        let contact_altitude = contact_ecef.to_geodetic().altitude.get();
        let penetration = (ground_elevation - contact_altitude) * normal_distance_scale;

        if !penetration.is_finite() || penetration <= 0.0 {
            continue;
        }

        let contact_velocity =
            state.velocity + state.orientation * state.angular_velocity.cross(contact_body);
        let penetration_rate = -contact_velocity.dot(ground_normal);

        let main_stroke = penetration.min(leg.max_stroke().get());
        let bottomed_stroke = (penetration - leg.max_stroke().get()).max(0.0);
        let elastic_force = leg.spring_rate().get()
            * (main_stroke + BOTTOM_OUT_STIFFNESS_MULTIPLIER * bottomed_stroke);
        let damping_force = leg.damping_coefficient().get() * penetration_rate;

        // 反発中は減衰がばね力を弱めるが、地面が脚を引っ張ることはない。
        let normal_force = (elastic_force + damping_force).max(0.0);
        if !normal_force.is_finite() || normal_force <= 0.0 {
            continue;
        }

        let mut force_ecef = ground_normal * normal_force;
        if has_tangent_basis {
            let forward_speed = contact_velocity.dot(forward_tangent);
            let lateral_speed = contact_velocity.dot(lateral_tangent);

            // Coulomb 摩擦の sign(v) は静止点で不連続になり、積分器が符号振動する。
            // tanh でゼロ近傍を線形化し、高速域では同じ Coulomb 上限へ漸近させる。
            let requested_friction = -forward_tangent
                * (forward_coefficient * (forward_speed / transition_speed).tanh())
                - lateral_tangent
                    * (gear.lateral_friction_coefficient()
                        * (lateral_speed / transition_speed).tanh());
            let requested_magnitude = requested_friction.length();
            let limited_friction = if requested_magnitude > friction_limit {
                requested_friction * (friction_limit / requested_magnitude)
            } else {
                requested_friction
            };
            force_ecef += limited_friction * normal_force;
        }

        let force_body = state.orientation.inverse() * force_ecef;
        total.force_body += force_body;
        total.moment_body += contact_body.cross(force_body);
    }

    total
}

/// 接触中または次の外部ステップ内に接触しそうかを判定する。
pub(crate) fn contact_is_active_or_imminent(
    gear: &LandingGearConfig,
    state: &RigidBodyState,
    environment: &Environment,
    frame: &LocalFrame,
    dt: Seconds,
) -> bool {
    if !ground_input_is_finite(environment) {
        return false;
    }

    let ground_frame = ground_frame(environment, frame);
    let normal = ground_normal_ecef(environment, &ground_frame);
    let normal_distance_scale = vertical_to_normal_distance_scale(environment);
    gear.legs().iter().any(|leg| {
        let contact_body = leg.contact_point().as_vec();
        let contact_ecef =
            Ecef::from_vec(state.position.as_vec() + state.orientation * contact_body);
        let offset_ned = ground_frame.ecef_to_ned_position(contact_ecef);
        let clearance = (contact_ecef.to_geodetic().altitude.get()
            - ground_elevation_at(environment, offset_ned))
            * normal_distance_scale;
        let contact_velocity =
            state.velocity + state.orientation * state.angular_velocity.cross(contact_body);
        let closing_distance = (-contact_velocity.dot(normal) * dt.get()).max(0.0);

        clearance <= closing_distance + 1.0e-3
    })
}

/// 接触時に考慮すべき最大固有角周波数 `rad/s`。
pub(crate) fn maximum_natural_frequency(
    gear: &LandingGearConfig,
    mass: &MassProperties,
    state: &RigidBodyState,
    environment: &Environment,
    frame: &LocalFrame,
) -> f64 {
    let ground_frame = ground_frame(environment, frame);
    let normal_body = state.orientation.inverse() * ground_normal_ecef(environment, &ground_frame);
    let inverse_mass = 1.0 / mass.mass().get();
    let mut total_effective_spring_rate = 0.0;
    let mut maximum: f64 = 0.0;

    for leg in gear.legs() {
        // 接触中に 1 外部ステップで底付きへ遷移しても分割不足にならないよう、
        // サブステップ判定には常に最大（バンプストップ）剛性を使う。
        let effective_spring_rate = leg.spring_rate().get() * BOTTOM_OUT_STIFFNESS_MULTIPLIER;
        total_effective_spring_rate += effective_spring_rate;

        let rotational_lever = leg.contact_point().as_vec().cross(normal_body);
        let inverse_effective_mass =
            inverse_mass + rotational_lever.dot(mass.inverse_inertia() * rotational_lever);
        let frequency = (effective_spring_rate * inverse_effective_mass).sqrt();
        maximum = maximum.max(frequency);
    }

    maximum.max((total_effective_spring_rate / mass.mass().get()).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AircraftConfig, GroundSlope};
    use flightsim_core::{Attitude, Geodetic, Meters, Ned};

    fn state_at_altitude(altitude: f64, velocity: Ned) -> RigidBodyState {
        RigidBodyState::from_geodetic(
            Geodetic::from_degrees(0.0, 0.0, altitude),
            Attitude::default(),
            velocity,
        )
    }

    #[test]
    fn no_contact_produces_no_load() {
        let config = AircraftConfig::light_single();
        let state = state_at_altitude(10.0, Ned::default());
        let environment = Environment::still_air();
        let frame = state.local_frame();
        let result = loads(
            &config.landing_gear,
            &state,
            ControlInputs::neutral(),
            &environment,
            &frame,
        );

        assert_eq!(result.force_body, DVec3::ZERO);
        assert_eq!(result.moment_body, DVec3::ZERO);
    }

    #[test]
    fn known_penetration_matches_hookes_law_and_is_symmetric() {
        let config = AircraftConfig::light_single();
        let penetration = 0.02;
        let state = state_at_altitude(1.0 - penetration, Ned::default());
        let environment = Environment::still_air();
        let frame = state.local_frame();
        let result = loads(
            &config.landing_gear,
            &state,
            ControlInputs::neutral(),
            &environment,
            &frame,
        );
        let expected = 3.0 * 120_000.0 * penetration;

        assert!(
            (result.force_body.z + expected).abs() < 0.1,
            "vertical force was {}, expected {}",
            result.force_body.z,
            -expected
        );
        assert!(result.force_body.x.abs() < 1.0e-9);
        assert!(result.force_body.y.abs() < 1.0e-9);
        assert!(result.moment_body.x.abs() < 0.1);
        assert!(result.moment_body.y.abs() < 0.1);
        assert!(result.moment_body.z.abs() < 0.1);

        let rebounding = state_at_altitude(1.0 - penetration, Ned::new(0.0, 0.0, -10.0));
        let rebound_load = loads(
            &config.landing_gear,
            &rebounding,
            ControlInputs::neutral(),
            &environment,
            &rebounding.local_frame(),
        );
        assert_eq!(
            rebound_load.force_body,
            DVec3::ZERO,
            "damping must not make the ground pull a rebounding aircraft downward"
        );

        let bottomed = state_at_altitude(0.5, Ned::default());
        let bottomed_load = loads(
            &config.landing_gear,
            &bottomed,
            ControlInputs::neutral(),
            &environment,
            &bottomed.local_frame(),
        );
        let expected_bottomed = 3.0 * 120_000.0 * (0.25 + BOTTOM_OUT_STIFFNESS_MULTIPLIER * 0.25);
        assert!(
            (bottomed_load.force_body.z + expected_bottomed).abs() < 1.0,
            "bottom-stop force was {}, expected {}",
            bottomed_load.force_body.z,
            -expected_bottomed
        );
    }

    #[test]
    fn brakes_oppose_forward_motion_without_exceeding_friction_limit() {
        let config = AircraftConfig::light_single();
        let state = state_at_altitude(0.98, Ned::new(10.0, 0.0, 0.0));
        let environment = Environment::still_air();
        let frame = state.local_frame();
        let free = loads(
            &config.landing_gear,
            &state,
            ControlInputs::neutral(),
            &environment,
            &frame,
        );
        let braking = loads(
            &config.landing_gear,
            &state,
            ControlInputs::neutral().with_brakes(1.0),
            &environment,
            &frame,
        );

        assert!(braking.force_body.x < free.force_body.x);
        assert!(braking.force_body.x < 0.0);
        assert!(braking.force_body.x.abs() <= -braking.force_body.z * 0.8 + 1.0e-9);

        let reverse = state_at_altitude(0.98, Ned::new(-10.0, 0.0, 0.0));
        let reverse_load = loads(
            &config.landing_gear,
            &reverse,
            ControlInputs::neutral().with_brakes(1.0),
            &environment,
            &reverse.local_frame(),
        );
        assert!(
            reverse_load.force_body.x > 0.0,
            "friction must oppose reverse motion"
        );

        let sideways = state_at_altitude(0.98, Ned::new(0.0, 10.0, 0.0));
        let sideways_load = loads(
            &config.landing_gear,
            &sideways,
            ControlInputs::neutral(),
            &environment,
            &sideways.local_frame(),
        );
        assert!(
            sideways_load.force_body.y < 0.0,
            "lateral friction had the wrong sign"
        );

        let slow_forward = state_at_altitude(0.98, Ned::new(1.0e-6, 0.0, 0.0));
        let slow_reverse = state_at_altitude(0.98, Ned::new(-1.0e-6, 0.0, 0.0));
        let slow_forward_load = loads(
            &config.landing_gear,
            &slow_forward,
            ControlInputs::neutral(),
            &environment,
            &slow_forward.local_frame(),
        );
        let slow_reverse_load = loads(
            &config.landing_gear,
            &slow_reverse,
            ControlInputs::neutral(),
            &environment,
            &slow_reverse.local_frame(),
        );
        assert!(slow_forward_load.force_body.x < 0.0);
        assert!(slow_reverse_load.force_body.x > 0.0);
        assert!(
            (slow_forward_load.force_body.x + slow_reverse_load.force_body.x).abs() < 1.0e-9,
            "regularized friction must be continuous and antisymmetric around rest"
        );
        assert!(
            slow_forward_load.force_body.x.abs() < braking.force_body.x.abs() * 1.0e-4,
            "near-zero friction did not enter the linearized region"
        );
    }

    #[test]
    fn slope_changes_the_load_between_front_and_main_gear() {
        let config = AircraftConfig::light_single();
        let state = state_at_altitude(0.98, Ned::default());
        let environment = Environment::still_air().with_ground_plane(
            state.geodetic(),
            Meters(0.0),
            GroundSlope::new(0.05, 0.0),
        );
        let frame = state.local_frame();
        let result = loads(
            &config.landing_gear,
            &state,
            ControlInputs::neutral(),
            &environment,
            &frame,
        );

        assert!(
            result.moment_body.y > 1.0,
            "uphill nose gear should create a nose-up moment"
        );
        assert!(result.force_body.is_finite() && result.moment_body.is_finite());

        let mut moved_uphill = state;
        moved_uphill.position = frame.ned_to_ecef_position(Ned::new(1.0, 0.0, 0.0));
        let moved_frame = moved_uphill.local_frame();
        let moved_result = loads(
            &config.landing_gear,
            &moved_uphill,
            ControlInputs::neutral(),
            &environment,
            &moved_frame,
        );
        assert!(
            moved_result.force_body.length() > result.force_body.length(),
            "the fixed ground reference did not make an uphill move increase penetration"
        );
    }

    #[test]
    fn contact_loads_stay_finite_at_poles_and_the_dateline() {
        let config = AircraftConfig::light_single();
        let environment = Environment::still_air();

        for (latitude, longitude) in [
            (-90.0, -180.0),
            (-90.0, 180.0),
            (0.0, -180.0),
            (0.0, 180.0),
            (90.0, -180.0),
            (90.0, 180.0),
        ] {
            let state = RigidBodyState::from_geodetic(
                Geodetic::from_degrees(latitude, longitude, 0.98),
                Attitude::default(),
                Ned::default(),
            );
            let frame = state.local_frame();
            let result = loads(
                &config.landing_gear,
                &state,
                ControlInputs::neutral(),
                &environment,
                &frame,
            );

            assert!(
                result.force_body.is_finite() && result.moment_body.is_finite(),
                "non-finite contact load at ({latitude}, {longitude})"
            );
            assert!(
                result.force_body.length() > 0.0,
                "gear did not contact at ({latitude}, {longitude})"
            );
        }
    }
}
