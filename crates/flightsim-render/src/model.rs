//! 外部 3D モデルを機体軸へ合わせる。
//!
//! # なぜ補正層が要るのか
//!
//! 調達してきたモデルの座標系は**こちらの都合を知らない**。前がどの軸か、
//! 上がどの軸か、大きさが何単位か、モデルによって全部違う。生成 AI が出した
//! モデルなら、そもそも一貫した規約が無い。
//!
//! そのまま読み込むと、機体が横倒しになる・逆さまになる・1000 倍の大きさで
//! 出る、といったことが起きる。**モデルを差し替えるたびに描画コードを
//! 書き換えるのではなく、ここで吸収する。**
//!
//! # 機体軸
//!
//! X = 前、Y = 右、Z = **下**（`flightsim-fdm` の規約）。
//! 一般的な 3D ツールは Y-up か Z-up で、Z が下を向くことはまず無い。
//! ここが最も間違えやすい。

use bevy::camera::primitives::Aabb;
use bevy::prelude::*;
use flightsim_core::Meters;

/// モデル座標系の軸。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelAxis {
    PositiveX,
    NegativeX,
    PositiveY,
    NegativeY,
    PositiveZ,
    NegativeZ,
}

impl ModelAxis {
    #[must_use]
    pub const fn to_vec3(self) -> Vec3 {
        match self {
            Self::PositiveX => Vec3::X,
            Self::NegativeX => Vec3::NEG_X,
            Self::PositiveY => Vec3::Y,
            Self::NegativeY => Vec3::NEG_Y,
            Self::PositiveZ => Vec3::Z,
            Self::NegativeZ => Vec3::NEG_Z,
        }
    }

    /// CLI や設定から読む。
    ///
    /// # Errors
    ///
    /// 知らない名前の場合。
    pub fn parse(text: &str) -> Result<Self, ModelFitError> {
        match text.trim().to_lowercase().as_str() {
            "+x" | "x" => Ok(Self::PositiveX),
            "-x" => Ok(Self::NegativeX),
            "+y" | "y" => Ok(Self::PositiveY),
            "-y" => Ok(Self::NegativeY),
            "+z" | "z" => Ok(Self::PositiveZ),
            "-z" => Ok(Self::NegativeZ),
            other => Err(ModelFitError::UnknownAxis(other.to_owned())),
        }
    }
}

/// 補正の設定が不正。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelFitError {
    UnknownAxis(String),
    /// 前と上が平行。回転を一意に決められない。
    DegenerateAxes,
}

impl core::fmt::Display for ModelFitError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownAxis(text) => write!(
                formatter,
                "`{text}` is not an axis; expected one of +x -x +y -y +z -z"
            ),
            Self::DegenerateAxes => write!(
                formatter,
                "the model's forward and up axes are parallel; they must be perpendicular"
            ),
        }
    }
}

impl std::error::Error for ModelFitError {}

/// モデル座標系から機体軸への補正。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelFit {
    /// モデルで機首が向いている軸。
    pub forward: ModelAxis,
    /// モデルで機体の上が向いている軸。
    pub up: ModelAxis,
    /// 合わせたい全長。モデルの寸法から倍率を自動で決める。
    pub target_length: Meters,
}

impl Default for ModelFit {
    fn default() -> Self {
        Self {
            // glTF の慣習は Y-up・-Z 前方。
            forward: ModelAxis::NegativeZ,
            up: ModelAxis::PositiveY,
            target_length: Meters(8.3),
        }
    }
}

impl ModelFit {
    /// # Errors
    ///
    /// 前と上が直交していない場合。
    pub fn new(
        forward: ModelAxis,
        up: ModelAxis,
        target_length: Meters,
    ) -> Result<Self, ModelFitError> {
        // 平行なら外積がほぼゼロになり、回転を作れない。
        if forward.to_vec3().cross(up.to_vec3()).length() < 0.5 {
            return Err(ModelFitError::DegenerateAxes);
        }
        Ok(Self {
            forward,
            up,
            target_length,
        })
    }

    /// モデル座標を機体軸へ写す回転。
    ///
    /// モデルの前が機体の +X、上が機体の **-Z** を向くようにする。
    #[must_use]
    pub fn rotation(&self) -> Quat {
        let forward = self.forward.to_vec3();
        let up = self.up.to_vec3();
        // 右手系では 前 × 上 = 右（glTF の -Z 前方・+Y 上で +X が右になる）。
        let right = forward.cross(up);

        // モデル基底を機体基底へ写す。基底は正規直交なので逆行列 = 転置。
        let model = Mat3::from_cols(forward, right, up);
        let body = Mat3::from_cols(Vec3::X, Vec3::Y, Vec3::NEG_Z);
        Quat::from_mat3(&(body * model.transpose())).normalize()
    }

    /// モデルの寸法から、目標全長に合わせる倍率を出す。
    ///
    /// **モデルの前後方向の長さ**を基準にする。生成モデルは大きさがまちまちなので、
    /// 固定倍率では合わない。
    ///
    /// 寸法が取れない・ゼロの場合は等倍にする。**0 除算で倍率が無限大になると、
    /// 機体が画面を埋め尽くして原因が分からなくなる。**
    #[must_use]
    pub fn scale_for(&self, model_extents: Vec3) -> f32 {
        let along_forward = (model_extents * self.forward.to_vec3()).length();
        #[allow(
            clippy::cast_possible_truncation,
            reason = "機体の全長は数メートル。f32 で十分"
        )]
        let target = self.target_length.get() as f32;

        if !along_forward.is_finite() || along_forward < 1e-6 || !target.is_finite() {
            return 1.0;
        }
        target / along_forward
    }

    /// 読み込んだモデルに適用する `Transform`。
    #[must_use]
    pub fn transform_for(&self, bounds: Aabb) -> Transform {
        let extents = Vec3::from(bounds.half_extents) * 2.0;
        Transform {
            translation: Vec3::ZERO,
            rotation: self.rotation(),
            scale: Vec3::splat(self.scale_for(extents)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// glTF の慣習（Y-up、-Z 前方）。
    fn gltf() -> ModelFit {
        ModelFit::new(ModelAxis::NegativeZ, ModelAxis::PositiveY, Meters(8.3))
            .expect("perpendicular axes")
    }

    // --- 向き ---

    #[test]
    fn the_model_nose_ends_up_pointing_forward() {
        // 機体軸の +X が前。ここを外すと機体が横向きや後ろ向きに飛ぶ。
        let fit = gltf();
        let nose = fit.rotation() * fit.forward.to_vec3();
        assert!(
            nose.dot(Vec3::X) > 0.999,
            "the model's nose maps to {nose:?} instead of +X"
        );
    }

    #[test]
    fn the_model_up_ends_up_pointing_up_in_body_axes() {
        // 機体軸の Z は**下向き**。上は -Z。
        // Y-up の感覚のままだと機体が横倒しになる。
        let fit = gltf();
        let up = fit.rotation() * fit.up.to_vec3();
        assert!(
            up.dot(Vec3::NEG_Z) > 0.999,
            "the model's up maps to {up:?} instead of -Z (body up)"
        );
    }

    #[test]
    fn the_model_right_ends_up_on_the_right() {
        let fit = gltf();
        let right = fit.rotation() * (fit.forward.to_vec3().cross(fit.up.to_vec3()));
        assert!(
            right.dot(Vec3::Y) > 0.999,
            "the model's right maps to {right:?} instead of +Y"
        );
    }

    #[test]
    fn the_result_is_a_rotation_not_a_reflection() {
        // 行列式が -1 だと左右が反転し、右旋回で左翼が下がる。
        for (forward, up) in [
            (ModelAxis::NegativeZ, ModelAxis::PositiveY),
            (ModelAxis::PositiveX, ModelAxis::PositiveZ),
            (ModelAxis::PositiveY, ModelAxis::NegativeX),
            (ModelAxis::PositiveZ, ModelAxis::PositiveY),
        ] {
            let fit = ModelFit::new(forward, up, Meters(8.0)).expect("perpendicular");
            let matrix = Mat3::from_quat(fit.rotation());
            assert!(
                matrix.determinant() > 0.99,
                "{forward:?}/{up:?} produced a reflection (determinant {})",
                matrix.determinant()
            );
            assert!((fit.rotation().length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn a_z_up_model_is_handled_too() {
        // Blender の既定は Z-up。glTF と違う。
        let fit = ModelFit::new(ModelAxis::PositiveY, ModelAxis::PositiveZ, Meters(8.0))
            .expect("perpendicular");
        let nose = fit.rotation() * Vec3::Y;
        let up = fit.rotation() * Vec3::Z;
        assert!(nose.dot(Vec3::X) > 0.999, "nose {nose:?}");
        assert!(up.dot(Vec3::NEG_Z) > 0.999, "up {up:?}");
    }

    #[test]
    fn parallel_axes_are_rejected() {
        // 平行だと回転を一意に決められない。黙って妙な向きにするより落とす。
        for (forward, up) in [
            (ModelAxis::PositiveX, ModelAxis::PositiveX),
            (ModelAxis::PositiveX, ModelAxis::NegativeX),
            (ModelAxis::NegativeZ, ModelAxis::PositiveZ),
        ] {
            assert_eq!(
                ModelFit::new(forward, up, Meters(8.0)),
                Err(ModelFitError::DegenerateAxes),
                "{forward:?}/{up:?} should have been rejected"
            );
        }
    }

    // --- 大きさ ---

    #[test]
    fn the_model_is_scaled_to_the_target_length() {
        // 生成モデルは大きさがまちまち。固定倍率では合わない。
        let fit = gltf();
        // 前後方向（-Z）に 2 m のモデル。
        let scale = fit.scale_for(Vec3::new(1.0, 0.5, 2.0));
        assert!(
            (scale - 8.3 / 2.0).abs() < 1e-4,
            "a 2 m model scaled by {scale}; expected {}",
            8.3 / 2.0
        );
    }

    #[test]
    fn a_huge_model_is_scaled_down() {
        let fit = gltf();
        let scale = fit.scale_for(Vec3::new(500.0, 200.0, 1_000.0));
        assert!(scale < 0.01, "a 1 km model scaled by {scale}");
    }

    #[test]
    fn a_degenerate_model_does_not_produce_an_infinite_scale() {
        // 0 除算で倍率が無限大になると、機体が画面を埋め尽くして
        // 原因が分からなくなる。
        let fit = gltf();
        for extents in [
            Vec3::ZERO,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(f32::NAN, 1.0, 1.0),
            Vec3::new(1.0, 1.0, f32::INFINITY),
        ] {
            let scale = fit.scale_for(extents);
            assert!(
                scale.is_finite() && scale > 0.0,
                "extents {extents:?} gave scale {scale}"
            );
        }
    }

    #[test]
    fn the_transform_carries_both_rotation_and_scale() {
        let fit = gltf();
        let bounds = Aabb {
            center: Vec3::ZERO.into(),
            half_extents: Vec3::new(0.5, 0.25, 1.0).into(),
        };
        let transform = fit.transform_for(bounds);

        assert!((transform.scale.x - 8.3 / 2.0).abs() < 1e-4);
        assert!(transform.translation.abs_diff_eq(Vec3::ZERO, 1e-6));
        assert!((transform.rotation.length() - 1.0).abs() < 1e-5);
    }

    // --- 設定の読み取り ---

    #[test]
    fn axes_are_parsed_from_text() {
        assert_eq!(ModelAxis::parse("+x"), Ok(ModelAxis::PositiveX));
        assert_eq!(ModelAxis::parse("X"), Ok(ModelAxis::PositiveX));
        assert_eq!(ModelAxis::parse(" -z "), Ok(ModelAxis::NegativeZ));
        assert!(matches!(
            ModelAxis::parse("up"),
            Err(ModelFitError::UnknownAxis(_))
        ));
    }

    #[test]
    fn the_error_says_what_was_expected() {
        // 「不正な軸」だけでは何を書けばいいか分からない。
        let message = ModelAxis::parse("forward")
            .expect_err("not an axis")
            .to_string();
        assert!(message.contains("+x"), "{message}");
    }
}
