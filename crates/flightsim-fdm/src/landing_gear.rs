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
///
/// **バンプストップの行程も有限**（[`LandingGearLeg::bottom_stop_travel`]）。
/// 使い切った先で弾性力は一定になる。青天井の線形ばねにすると、深いめり込みが
/// 深さの 2 乗のエネルギーとして蓄えられ、伸びるときにそれが機体へ返る。
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

/// 1 本の脚が地面から受ける法線方向の力 `N`。
///
/// `penetration` は接地点の地面へのめり込み量 `m`、`penetration_rate` はその増加率 `m/s`
/// （正が圧縮、負が伸長）。
///
/// # 3 つの性質
///
/// 1. **地面は脚を引っ張らない。** 結果は常に非負。
/// 2. **弾性力は有限。** 主ばね行程 + バンプストップ行程を使い切った先では一定になる。
/// 3. **伸長速度に上限がある。** 接地点が `max_recoil_speed` より速く離れようとすると
///    弾性力が抜ける。これがないと、深くめり込んだ状態から脚が伸びる間に
///    蓄えたエネルギーを一気に返し、機体を跳ね上げて裏返す。
///
/// 3 が効くのは「めり込んだ状態で初期化された」場合で、そのめり込みは機体の運動で
/// 生じたものではないため、蓄えられている弾性エネルギーは**そもそも物理的な裏付けがない**。
/// 実機のオレオもリコイル弁で戻りを絞っており、跳ね返らないのが正しい挙動になる。
///
/// なお、抜けるのは弾性項だけで減衰項は残る。減衰項は伸長中は負なので、
/// 非負クランプと合わせて「離れるときは力ゼロ」に収束する。
fn normal_force(leg: &crate::LandingGearLeg, penetration: f64, penetration_rate: f64) -> f64 {
    // 壊れた状態から NaN / Inf を撒かない。ここで止めないと全状態へ伝播する。
    // 速度が非有限なら機体はすでに発散しており、力を足しても意味がない。
    if !penetration.is_finite() || !penetration_rate.is_finite() {
        return 0.0;
    }

    let main_stroke = penetration.min(leg.max_stroke().get());
    let bottom_stop_stroke =
        (penetration - leg.max_stroke().get()).clamp(0.0, leg.bottom_stop_travel().get());
    let elastic_force = leg.spring_rate().get()
        * (main_stroke + BOTTOM_OUT_STIFFNESS_MULTIPLIER * bottom_stop_stroke);

    // 伸長速度が上限に達するまで線形に弾性力を落とす。段差にすると
    // RK4 の中間評価で力が飛び、サブステップ数を増やしても収束しない。
    //
    // `f64::clamp` は NaN を素通りさせるが、`penetration_rate` の有限性は上で確認済み、
    // `max_recoil_speed` は構築時に有限な正値だと保証されているので、ここで NaN にはならない。
    let recoil_speed = (-penetration_rate).max(0.0);
    let recoil_fade = (1.0 - recoil_speed / leg.max_recoil_speed().get()).clamp(0.0, 1.0);

    let damping_force = leg.damping_coefficient().get() * penetration_rate;
    (elastic_force * recoil_fade + damping_force).max(0.0)
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

        let normal_force = normal_force(leg, penetration, penetration_rate);
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

        // バンプストップの途中。主ばね全ストローク + 超過分 × 剛性倍率。
        let leg = config.landing_gear.legs()[0];
        let part_way = leg.max_stroke().get() + leg.bottom_stop_travel().get() * 0.5;
        let bottoming = state_at_altitude(1.0 - part_way, Ned::default());
        let bottoming_load = loads(
            &config.landing_gear,
            &bottoming,
            ControlInputs::neutral(),
            &environment,
            &bottoming.local_frame(),
        );
        let expected_bottoming = 3.0
            * 120_000.0
            * (leg.max_stroke().get()
                + BOTTOM_OUT_STIFFNESS_MULTIPLIER * leg.bottom_stop_travel().get() * 0.5);
        assert!(
            (bottoming_load.force_body.z + expected_bottoming).abs() < 1.0,
            "bottom-stop force was {}, expected {}",
            bottoming_load.force_body.z,
            -expected_bottoming
        );

        // 行程を使い切った先では弾性力が一定になる。**青天井の線形ばねにしないこと。**
        // 深さの 2 乗でエネルギーが溜まり、脚が伸びるときに機体を裏返す仕事に化ける。
        let saturated = 3.0
            * 120_000.0
            * (leg.max_stroke().get()
                + BOTTOM_OUT_STIFFNESS_MULTIPLIER * leg.bottom_stop_travel().get());
        for depth in [
            leg.max_stroke().get() + leg.bottom_stop_travel().get(),
            0.5,
            5.0,
            500.0,
        ] {
            let deep = state_at_altitude(1.0 - depth, Ned::default());
            let deep_load = loads(
                &config.landing_gear,
                &deep,
                ControlInputs::neutral(),
                &environment,
                &deep.local_frame(),
            );
            assert!(
                (deep_load.force_body.z + saturated).abs() < 1.0,
                "elastic force at {depth} m of penetration was {}, expected the saturated {}",
                deep_load.force_body.z,
                -saturated
            );
        }
    }

    #[test]
    fn the_elastic_force_fades_out_above_the_recoil_speed_limit() {
        // 深くめり込んだ状態から脚が伸びるとき、蓄えた弾性エネルギーが一気に返ると
        // 機体が跳ね上がる。伸長速度が上限を超えたら弾性項を止める。
        let leg = AircraftConfig::light_single().landing_gear.legs()[0];
        let deep = 1.0;
        let limit = leg.max_recoil_speed().get();

        let static_force = normal_force(&leg, deep, 0.0);
        assert!(static_force > 0.0);

        // 上限のちょうど半分の伸長速度で、弾性項は半分になる（減衰項ぶん更に下がる）。
        let half = normal_force(&leg, deep, -limit * 0.5);
        assert!(half < static_force * 0.5 + 1.0e-9);
        assert!(half > 0.0, "the gear must still push while it is recoiling");

        // 上限に達したら押し返さない。負にもならない（地面は脚を引かない）。
        for speed in [limit, limit * 2.0, 1.0e6] {
            let faded = normal_force(&leg, deep, -speed);
            assert!(
                faded == 0.0,
                "the gear kept pushing at {speed} m/s of recoil: {faded} N"
            );
        }

        // 圧縮側は変わっていないこと。
        let compressing = normal_force(&leg, 0.02, 1.0);
        assert!(
            (compressing - (120_000.0 * 0.02 + 13_000.0)).abs() < 1.0e-6,
            "compression-side force changed: {compressing}"
        );
    }

    #[test]
    fn a_non_finite_penetration_rate_does_not_produce_a_non_finite_force() {
        // NaN は全状態へ伝播する。ここで止める。
        let leg = AircraftConfig::light_single().landing_gear.legs()[0];
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let force = normal_force(&leg, 0.1, rate);
            assert!(
                force.is_finite(),
                "penetration rate {rate} produced {force}"
            );
            assert!(force >= 0.0);
        }
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
