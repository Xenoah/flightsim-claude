//! 機体設定。質量特性・幾何・空力係数・エンジン・着陸装置。
//!
//! 係数をコードに直書きせず設定として外に出しているのは、機体を差し替えられるようにするため。
//! 将来これを設定ファイルから読み込む形にする（インタフェースはそのままで済む）。

use flightsim_core::{Kilograms, Meters, MetersPerSecond, Newtons, Radians, SquareMeters};
use glam::{DMat3, DVec3};

/// 機体重心を原点とする機体軸上の点。
///
/// X は前、Y は右、Z は下。各成分の単位はメートル。
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(transparent)]
pub struct BodyPoint(DVec3);

impl BodyPoint {
    #[must_use]
    pub const fn new(x: Meters, y: Meters, z: Meters) -> Self {
        Self(DVec3::new(x.get(), y.get(), z.get()))
    }

    #[must_use]
    pub const fn as_vec(self) -> DVec3 {
        self.0
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }
}

/// 着陸脚のばね定数 `N/m`。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct SpringRate(pub f64);

impl SpringRate {
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// 着陸脚の粘性減衰係数 `N·s/m`。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct DampingCoefficient(pub f64);

impl DampingCoefficient {
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// 1 本の着陸脚。
#[derive(Debug, Clone, Copy)]
pub struct LandingGearLeg {
    /// 脚を伸ばし切った状態の接地点。
    contact_point: BodyPoint,
    spring_rate: SpringRate,
    damping_coefficient: DampingCoefficient,
    max_stroke: Meters,
}

impl LandingGearLeg {
    /// # Panics
    ///
    /// 接地点または脚の物理量が有限でない場合、ばね定数・最大ストロークが正でない場合、
    /// 減衰係数が負の場合にパニックする。
    #[must_use]
    pub fn new(
        contact_point: BodyPoint,
        spring_rate: SpringRate,
        damping_coefficient: DampingCoefficient,
        max_stroke: Meters,
    ) -> Self {
        assert!(
            contact_point.is_finite(),
            "gear contact point must be finite"
        );
        assert!(
            spring_rate.get().is_finite() && spring_rate.get() > 0.0,
            "gear spring rate must be finite and positive"
        );
        assert!(
            damping_coefficient.get().is_finite() && damping_coefficient.get() >= 0.0,
            "gear damping coefficient must be finite and non-negative"
        );
        assert!(
            max_stroke.is_finite() && max_stroke.get() > 0.0,
            "gear maximum stroke must be finite and positive"
        );

        Self {
            contact_point,
            spring_rate,
            damping_coefficient,
            max_stroke,
        }
    }

    #[must_use]
    pub const fn contact_point(&self) -> BodyPoint {
        self.contact_point
    }

    #[must_use]
    pub const fn spring_rate(&self) -> SpringRate {
        self.spring_rate
    }

    #[must_use]
    pub const fn damping_coefficient(&self) -> DampingCoefficient {
        self.damping_coefficient
    }

    #[must_use]
    pub const fn max_stroke(&self) -> Meters {
        self.max_stroke
    }
}

/// 3 点式着陸装置とタイヤ摩擦の設定。
#[derive(Debug, Clone, Copy)]
pub struct LandingGearConfig {
    /// 前脚 1 本と主脚 2 本。順序に物理的な意味は持たせない。
    legs: [LandingGearLeg; 3],
    /// 自由転動中の前後方向摩擦係数。
    rolling_friction_coefficient: f64,
    /// ブレーキ全開時に加算する前後方向摩擦係数。
    braking_friction_coefficient: f64,
    /// 横滑りを拘束する摩擦係数。
    lateral_friction_coefficient: f64,
    /// 摩擦を Coulomb 上限へ滑らかにつなぐ速度幅。
    friction_transition_speed: MetersPerSecond,
}

impl LandingGearConfig {
    /// # Panics
    ///
    /// 摩擦係数が有限な非負値でない場合、または遷移速度が有限な正値でない場合に
    /// パニックする。
    #[must_use]
    pub fn new(
        legs: [LandingGearLeg; 3],
        rolling_friction_coefficient: f64,
        braking_friction_coefficient: f64,
        lateral_friction_coefficient: f64,
        friction_transition_speed: MetersPerSecond,
    ) -> Self {
        for (name, coefficient) in [
            ("rolling", rolling_friction_coefficient),
            ("braking", braking_friction_coefficient),
            ("lateral", lateral_friction_coefficient),
        ] {
            assert!(
                coefficient.is_finite() && coefficient >= 0.0,
                "{name} friction coefficient must be finite and non-negative"
            );
        }
        assert!(
            friction_transition_speed.is_finite() && friction_transition_speed.get() > 0.0,
            "friction transition speed must be finite and positive"
        );

        Self {
            legs,
            rolling_friction_coefficient,
            braking_friction_coefficient,
            lateral_friction_coefficient,
            friction_transition_speed,
        }
    }

    /// 軽単発機向けの前輪式 3 点着陸装置。
    #[must_use]
    pub fn light_single() -> Self {
        let leg = |x, y| {
            LandingGearLeg::new(
                BodyPoint::new(Meters(x), Meters(y), Meters(1.0)),
                SpringRate(120_000.0),
                DampingCoefficient(13_000.0),
                Meters(0.25),
            )
        };

        Self::new(
            [leg(1.6, 0.0), leg(-0.8, -1.3), leg(-0.8, 1.3)],
            0.015,
            0.70,
            0.80,
            MetersPerSecond(0.25),
        )
    }

    #[must_use]
    pub const fn legs(&self) -> &[LandingGearLeg; 3] {
        &self.legs
    }

    #[must_use]
    pub const fn rolling_friction_coefficient(&self) -> f64 {
        self.rolling_friction_coefficient
    }

    #[must_use]
    pub const fn braking_friction_coefficient(&self) -> f64 {
        self.braking_friction_coefficient
    }

    #[must_use]
    pub const fn lateral_friction_coefficient(&self) -> f64 {
        self.lateral_friction_coefficient
    }

    #[must_use]
    pub const fn friction_transition_speed(&self) -> MetersPerSecond {
        self.friction_transition_speed
    }
}

/// 機体軸まわりの質量特性。
///
/// 慣性テンソルは機体軸（X 前・Y 右・Z 下）で表す。
/// 航空機は XZ 平面に対して概ね左右対称なので、非対角成分は `Ixz` のみを持つ。
#[derive(Debug, Clone, Copy)]
pub struct MassProperties {
    mass: Kilograms,
    inertia: DMat3,
    inverse_inertia: DMat3,
}

impl MassProperties {
    /// 慣性モーメントから構築する。単位は kg·m²。
    ///
    /// # Panics
    ///
    /// 質量または主慣性モーメントが正でない場合、慣性テンソルが特異な場合にパニックする。
    /// これらは物理的にあり得ない設定であり、静かに NaN を撒くより即座に落とすほうがよい。
    #[must_use]
    pub fn new(
        mass: Kilograms,
        moment_xx: f64,
        moment_yy: f64,
        moment_zz: f64,
        product_xz: f64,
    ) -> Self {
        assert!(mass.get() > 0.0, "mass must be positive, got {mass}");
        assert!(
            moment_xx > 0.0 && moment_yy > 0.0 && moment_zz > 0.0,
            "principal moments of inertia must be positive, got ({moment_xx}, {moment_yy}, {moment_zz})"
        );

        // 慣性テンソル。慣性乗積は定義上マイナス符号で入る。
        let inertia = DMat3::from_cols_array(&[
            moment_xx,
            0.0,
            -product_xz, // 第 1 列
            0.0,
            moment_yy,
            0.0, // 第 2 列
            -product_xz,
            0.0,
            moment_zz, // 第 3 列
        ]);

        let determinant = inertia.determinant();
        assert!(
            determinant.abs() > f64::EPSILON,
            "inertia tensor is singular (determinant {determinant}); \
             check that the product of inertia is smaller than the principal moments"
        );

        Self {
            mass,
            inertia,
            inverse_inertia: inertia.inverse(),
        }
    }

    #[must_use]
    pub const fn mass(&self) -> Kilograms {
        self.mass
    }

    #[must_use]
    pub const fn inertia(&self) -> DMat3 {
        self.inertia
    }

    /// 角加速度の計算で使う逆行列。構築時に一度だけ計算している。
    #[must_use]
    pub const fn inverse_inertia(&self) -> DMat3 {
        self.inverse_inertia
    }
}

/// 空力の基準となる幾何量。
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    /// 主翼面積 S。動圧に掛けて力にする基準面積。
    pub wing_area: SquareMeters,
    /// 翼幅 b。ロール・ヨーのモーメント基準長。
    pub wing_span: Meters,
    /// 平均空力翼弦 c̄。ピッチのモーメント基準長。
    pub mean_chord: Meters,
}

impl Geometry {
    /// アスペクト比 b²/S。誘導抗力の計算に使う。
    #[must_use]
    pub fn aspect_ratio(&self) -> f64 {
        let span = self.wing_span.get();
        span * span / self.wing_area.get()
    }
}

/// 無次元空力係数。
///
/// # 命名について
///
/// 航空の慣習では揚力係数を `C_L`、ロールモーメント係数を `C_l` と書き分けるが、
/// 大文字小文字だけの区別は Rust の識別子として危険（取り違えても読んで気づけない）。
/// ここでは軸の名前を綴って区別する。
///
/// # 符号
///
/// 操縦舵の効き（`roll_aileron` など）は**操縦指示の向き**で表す。
/// 教科書の舵面変位角基準の値とは符号が異なる。詳細は [`crate::ControlInputs`] を参照。
///
/// # 操縦舵係数の基準量
///
/// **操縦舵の係数は「正規化入力 `[-1, 1]` あたり」であって「1 ラジアンの舵角あたり」ではない。**
///
/// 安定微係数の教科書・データベースの値（`C_lδa = 0.178` など）は舵角 1 rad あたりで
/// 定義されている。これを `[-1, 1]` の入力にそのまま掛けると、舵角 57.3° に相当する
/// 過大な効きになる。実際の最大舵角は補助翼で約 20°、昇降舵で約 25°、方向舵で約 24°。
///
/// 教科書の値を写す際は **最大舵角［rad］を掛けてから** ここに入れること。
/// この取り違えは「フルエルロンで毎秒 217° ロールする」という形で現れた。
#[derive(Debug, Clone, Copy)]
pub struct AeroCoefficients {
    // --- 揚力 ---
    /// 迎角ゼロでの揚力係数。
    pub lift_zero: f64,
    /// 揚力傾斜 `1/rad`。
    pub lift_alpha: f64,
    /// フラップ全開時の揚力増加。
    pub lift_flaps: f64,
    /// 失速迎角。これを超えると平板理論へブレンドされる。
    pub stall_angle: Radians,
    /// 失速遷移の鋭さ。大きいほど急峻。典型値 50。
    pub stall_blend_rate: f64,

    // --- 抗力 ---
    /// 有害抗力係数（揚力に依存しない分）。
    pub drag_min: f64,
    /// Oswald 効率。誘導抗力 `C_L² / (π e AR)` の分母に入る。典型値 0.7〜0.85。
    pub oswald_efficiency: f64,
    /// フラップ全開時の抗力増加。
    pub drag_flaps: f64,

    // --- 横力（機体 Y 軸）---
    /// 横滑り角に対する横力。負（右からの相対風は左向きの力を生む）。
    pub side_beta: f64,
    /// 方向舵による横力。
    pub side_rudder: f64,

    // --- ロールモーメント（機体 X 軸まわり）---
    /// 上反角効果。負（右横滑りで左ロール）。
    pub roll_beta: f64,
    /// ロール減衰。負。
    pub roll_rate_p: f64,
    /// ヨーレートによるロール。正。
    pub roll_rate_r: f64,
    /// 補助翼の効き。正（右ロール指示で右ロール）。
    pub roll_aileron: f64,
    /// 方向舵によるロール。
    pub roll_rudder: f64,

    // --- ピッチモーメント（機体 Y 軸まわり）---
    /// 迎角ゼロでのピッチモーメント。
    pub pitch_zero: f64,
    /// 縦静安定。**負でなければならない。**正だと機体が発散する。
    pub pitch_alpha: f64,
    /// ピッチ減衰。負。
    pub pitch_rate_q: f64,
    /// 昇降舵の効き。正（機首上げ指示で機首上げ）。
    pub pitch_elevator: f64,
    /// フラップによるピッチモーメント変化。
    pub pitch_flaps: f64,

    // --- ヨーモーメント（機体 Z 軸まわり）---
    /// 風見安定。**正でなければならない。**負だと機体が横を向き続ける。
    pub yaw_beta: f64,
    /// ロールレートによるヨー。負。
    pub yaw_rate_p: f64,
    /// ヨー減衰。負。
    pub yaw_rate_r: f64,
    /// 補助翼による逆ヨー。負。
    pub yaw_aileron: f64,
    /// 方向舵の効き。正（右ヨー指示で右ヨー）。
    pub yaw_rudder: f64,
}

/// 単発プロペラ機のエンジン。
///
/// # 単純化していること
///
/// 定出力モデル（`推力 = 出力 / 速度`）に静止推力の上限を掛けたもの。
/// 実際のプロペラ効率は前進率とピッチによって変化し、混合比・過給・回転数の
/// 動特性も無視している。M1 で飛ばすには十分だが、**エンジン計器を実装する段階で
/// 作り直しが必要**。
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    /// 最大軸出力 `W`。
    pub max_shaft_power: f64,
    /// プロペラ効率。典型値 0.75〜0.85。
    pub propeller_efficiency: f64,
    /// 静止推力の上限。低速で推力が発散するのを防ぐ。
    pub static_thrust: Newtons,
}

impl EngineConfig {
    /// 与えられた対気速度・空気密度比における推力。
    ///
    /// 密度比に比例させているのは、高高度・高温で推力が落ちる効果を入れるため。
    #[must_use]
    pub fn thrust(&self, throttle: f64, true_airspeed: f64, density_ratio: f64) -> Newtons {
        let available_power =
            throttle.clamp(0.0, 1.0) * self.max_shaft_power * self.propeller_efficiency;

        // 速度ゼロで推力が発散するため、静止推力で頭打ちにする。
        // 分母の下限は「この速度以下では静止推力と等しい」という意味を持つ。
        let reference_speed = available_power / self.static_thrust.get().max(f64::EPSILON);
        let effective_speed = true_airspeed.max(reference_speed).max(f64::EPSILON);

        Newtons(available_power / effective_speed * density_ratio.max(0.0))
    }
}

/// 機体一式の設定。
#[derive(Debug, Clone)]
pub struct AircraftConfig {
    pub name: String,
    pub mass_properties: MassProperties,
    pub geometry: Geometry,
    pub aero: AeroCoefficients,
    pub engine: EngineConfig,
    pub landing_gear: LandingGearConfig,
}

impl AircraftConfig {
    /// 軽single機の代表的な諸元。
    ///
    /// # 注意
    ///
    /// **特定の実在機の認証データではない。**軽single機として妥当な範囲の
    /// 代表値を組み合わせたもので、挙動の目安として使うこと。
    /// 実機を再現する段階では、機種ごとの風洞・飛行試験データで置き換える。
    #[must_use]
    pub fn light_single() -> Self {
        Self {
            name: "Light Single (generic)".to_owned(),
            mass_properties: MassProperties::new(
                Kilograms(1_043.0),
                1_285.0,
                1_825.0,
                2_667.0,
                0.0,
            ),
            geometry: Geometry {
                wing_area: SquareMeters(16.17),
                wing_span: Meters(11.0),
                mean_chord: Meters(1.49),
            },
            aero: AeroCoefficients {
                lift_zero: 0.31,
                lift_alpha: 5.143,
                lift_flaps: 0.65,
                stall_angle: Radians(16.0_f64.to_radians()),
                stall_blend_rate: 50.0,

                drag_min: 0.031,
                oswald_efficiency: 0.75,
                drag_flaps: 0.06,

                // 操縦舵の係数は最大舵角を掛けて正規化入力あたりに換算済み。
                // 補助翼 20° = 0.349 rad、昇降舵 25° = 0.436 rad、方向舵 24° = 0.419 rad。
                side_beta: -0.31,
                side_rudder: 0.187 * 0.419,

                roll_beta: -0.089,
                roll_rate_p: -0.47,
                roll_rate_r: 0.096,
                roll_aileron: 0.178 * 0.349,
                roll_rudder: 0.0147 * 0.419,

                pitch_zero: 0.015,
                pitch_alpha: -0.89,
                pitch_rate_q: -12.4,
                pitch_elevator: 1.28 * 0.436,
                pitch_flaps: -0.12,

                yaw_beta: 0.065,
                yaw_rate_p: -0.03,
                yaw_rate_r: -0.099,
                // 逆ヨーは補助翼のロール効きの約 3% が目安（C_nδa / C_lδa ≒ -0.03）。
                // ここを一桁大きくすると、横操舵しただけで機首が 50°/s で振られ、
                // 発生した横滑りが上反角効果でロールを打ち消してしまう。
                yaw_aileron: -0.0053 * 0.349,
                yaw_rudder: 0.0657 * 0.419,
            },
            engine: EngineConfig {
                // 160 hp。
                max_shaft_power: 119_000.0,
                propeller_efficiency: 0.8,
                static_thrust: Newtons(2_400.0),
            },
            landing_gear: LandingGearConfig::light_single(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_ratio_matches_the_definition() {
        let geometry = Geometry {
            wing_area: SquareMeters(16.17),
            wing_span: Meters(11.0),
            mean_chord: Meters(1.49),
        };
        // b²/S = 121 / 16.17 ≒ 7.483
        assert!((geometry.aspect_ratio() - 7.483).abs() < 0.01);
    }

    #[test]
    fn inertia_inverse_is_a_true_inverse() {
        let mass = MassProperties::new(Kilograms(1_043.0), 1_285.0, 1_825.0, 2_667.0, 120.0);
        let product = mass.inertia() * mass.inverse_inertia();

        for (row, expected) in [
            (product.x_axis, glam::DVec3::X),
            (product.y_axis, glam::DVec3::Y),
            (product.z_axis, glam::DVec3::Z),
        ] {
            assert!(
                row.distance(expected) < 1e-9,
                "I · I⁻¹ is not the identity: {row} vs {expected}"
            );
        }
    }

    #[test]
    fn inertia_tensor_is_symmetric() {
        // 慣性テンソルは定義上対称。非対称だと角運動量が保存しない。
        let mass = MassProperties::new(Kilograms(1_000.0), 1_200.0, 1_800.0, 2_600.0, 90.0);
        let i = mass.inertia();
        assert!((i.x_axis.z - i.z_axis.x).abs() < 1e-12);
        assert!((i.x_axis.y - i.y_axis.x).abs() < 1e-12);
        assert!((i.y_axis.z - i.z_axis.y).abs() < 1e-12);
    }

    #[test]
    #[should_panic(expected = "mass must be positive")]
    fn zero_mass_is_rejected() {
        let _ = MassProperties::new(Kilograms(0.0), 1.0, 1.0, 1.0, 0.0);
    }

    #[test]
    #[should_panic(expected = "principal moments of inertia must be positive")]
    fn zero_inertia_is_rejected() {
        let _ = MassProperties::new(Kilograms(1.0), 0.0, 1.0, 1.0, 0.0);
    }

    #[test]
    fn default_landing_gear_is_symmetric_about_the_longitudinal_axis() {
        let gear = LandingGearConfig::light_single();
        let left = gear.legs()[1].contact_point().as_vec();
        let right = gear.legs()[2].contact_point().as_vec();

        assert!((left.x - right.x).abs() < f64::EPSILON);
        assert!((left.y + right.y).abs() < f64::EPSILON);
        assert!((left.z - right.z).abs() < f64::EPSILON);
    }

    #[test]
    #[should_panic(expected = "gear spring rate must be finite and positive")]
    fn invalid_landing_gear_spring_rate_is_rejected() {
        let _ = LandingGearLeg::new(
            BodyPoint::new(Meters(0.0), Meters(0.0), Meters(1.0)),
            SpringRate(f64::NAN),
            DampingCoefficient(1.0),
            Meters(0.2),
        );
    }

    // --- エンジン ---

    #[test]
    fn thrust_is_capped_at_static_thrust_when_stationary() {
        let engine = AircraftConfig::light_single().engine;
        let stationary = engine.thrust(1.0, 0.0, 1.0);

        assert!(stationary.is_finite(), "thrust diverged at zero airspeed");
        assert!(
            stationary.get() <= engine.static_thrust.get() + 1e-6,
            "static thrust cap was exceeded: {stationary}"
        );
        assert!(stationary.get() > 0.0);
    }

    #[test]
    fn thrust_decreases_with_airspeed() {
        // 定出力モデルなので、速度が上がると推力は下がる。
        let engine = AircraftConfig::light_single().engine;
        let slow = engine.thrust(1.0, 30.0, 1.0);
        let fast = engine.thrust(1.0, 70.0, 1.0);
        assert!(
            fast.get() < slow.get(),
            "thrust should fall as airspeed rises"
        );
    }

    #[test]
    fn thrust_decreases_with_altitude() {
        // 密度比 0.7（約 3 000 m 相当）で推力が落ちること。
        let engine = AircraftConfig::light_single().engine;
        let sea_level = engine.thrust(1.0, 50.0, 1.0);
        let altitude = engine.thrust(1.0, 50.0, 0.7);
        assert!(altitude.get() < sea_level.get());
    }

    #[test]
    fn idle_throttle_produces_no_thrust() {
        let engine = AircraftConfig::light_single().engine;
        assert!(engine.thrust(0.0, 50.0, 1.0).get().abs() < 1e-9);
    }

    #[test]
    fn thrust_never_goes_negative_or_nan() {
        let engine = AircraftConfig::light_single().engine;
        for throttle in [-1.0, 0.0, 0.5, 1.0, 2.0] {
            for airspeed in [0.0, 1e-12, 50.0, 1e6] {
                for density_ratio in [0.0, 0.1, 1.0, 1.5] {
                    let t = engine.thrust(throttle, airspeed, density_ratio);
                    assert!(
                        t.is_finite() && t.get() >= 0.0,
                        "thrust {t} for throttle {throttle}, airspeed {airspeed}, density {density_ratio}"
                    );
                }
            }
        }
    }

    // --- 係数の符号 ---

    #[test]
    fn default_coefficients_describe_a_stable_aircraft() {
        let aero = AircraftConfig::light_single().aero;

        // 縦静安定: 迎角が増えると機首下げモーメントが出る。
        assert!(
            aero.pitch_alpha < 0.0,
            "pitch_alpha must be negative for static stability"
        );
        // 風見安定: 右横滑りで機首が右を向き、横滑りが解消される。
        assert!(
            aero.yaw_beta > 0.0,
            "yaw_beta must be positive for weathercock stability"
        );
        // 上反角効果: 右横滑りで左ロール。
        assert!(
            aero.roll_beta < 0.0,
            "roll_beta must be negative for dihedral effect"
        );
        // 各軸の減衰は全て負。
        assert!(aero.roll_rate_p < 0.0, "roll damping must be negative");
        assert!(aero.pitch_rate_q < 0.0, "pitch damping must be negative");
        assert!(aero.yaw_rate_r < 0.0, "yaw damping must be negative");
        // 操縦指示の向き（正の入力が正のモーメントを生む）。
        assert!(aero.roll_aileron > 0.0);
        assert!(aero.pitch_elevator > 0.0);
        assert!(aero.yaw_rudder > 0.0);
        // 逆ヨー: 右ロール操作は左ヨーを誘発する。
        assert!(aero.yaw_aileron < 0.0, "adverse yaw should be present");
    }
}
