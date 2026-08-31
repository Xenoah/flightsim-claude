//! DEM の鉛直基準。
//!
//! # なぜ要るのか
//!
//! **標高には「どこから測ったか」が要る。** このプロジェクトの
//! `Geodetic::altitude` は WGS84 **楕円体高**が契約（ADR-0002）だが、
//! 実世界の DEM の多くは**ジオイド**（平均海面に近い等ポテンシャル面）を
//! 基準にした正標高で配布される。
//!
//! Copernicus DEM GLO-30 の鉛直基準は EGM2008（EPSG:3855）。
//! これを変換せずに楕円体高として扱うと、**ジオイド高ぶんの系統誤差**が
//! そのまま入る。日本付近では約 +30〜+40 m、地球全体では −107〜+86 m。
//!
//! 局所的には気付きにくい。滑走路も機体も同じだけずれるので、
//! **描画も接地も辻褄が合ってしまう。** 効くのは絶対高度と ECEF 半径で、
//! 高度計の指示、気圧高度との突き合わせ、他の測位源との統合が狂う。
//!
//! # ここで扱う範囲
//!
//! **識別と、黙って誤用しないこと**までを扱う。実際のジオイド高の
//! 適用（EGM2008 の展開、またはジオイド格子の読み込み）は、
//! データ源とその権利を決めてからの別の作業。
//!
//! 「変換できないので楕円体高として使う」を**利用者が明示的に選んだ**
//! ときだけ通す。既定では拒否する。

use std::fmt;

/// GeoTIFF の `VerticalCSTypeGeoKey`。
pub const VERTICAL_CS_TYPE_GEO_KEY: u16 = 4096;

/// EPSG:4979 — WGS84 の 3 次元測地座標系。高さは楕円体高。
pub const EPSG_WGS84_ELLIPSOIDAL_3D: u16 = 4979;

/// EPSG:5030 — WGS84 楕円体を鉛直基準として使う。
pub const EPSG_WGS84_ELLIPSOID_HEIGHT: u16 = 5030;

/// EPSG:3855 — EGM2008 ジオイド高。**Copernicus DEM GLO-30 の基準。**
pub const EPSG_EGM2008_HEIGHT: u16 = 3855;

/// EPSG:5773 — EGM96 ジオイド高。
pub const EPSG_EGM96_HEIGHT: u16 = 5773;

/// EPSG:5714 — 平均海面。どのモデルかは示されない。
pub const EPSG_MEAN_SEA_LEVEL: u16 = 5714;

/// DEM の高さが何を基準にしているか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalDatum {
    /// WGS84 楕円体高。**このプロジェクトの契約と一致する。**
    Ellipsoidal,
    /// ジオイド基準の正標高。楕円体高へ直すにはジオイド高が要る。
    Geoid(GeoidModel),
    /// GeoTIFF が鉛直基準を書いていない。
    ///
    /// **「楕円体高だろう」と決めつけない。** 実務では、書いていない
    /// DEM のほうがジオイド基準であることが多い。
    Unspecified,
    /// 識別できた EPSG コードだが、このプロジェクトが扱いを決めていない。
    Unsupported(u16),
}

/// ジオイドモデルの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoidModel {
    /// EGM2008。Copernicus DEM の基準。
    Egm2008,
    /// EGM96。
    Egm96,
    /// 平均海面としか書かれていない。**どのモデルか分からない。**
    UnspecifiedMeanSeaLevel,
}

impl GeoidModel {
    /// 人が読む名前。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Egm2008 => "EGM2008",
            Self::Egm96 => "EGM96",
            Self::UnspecifiedMeanSeaLevel => "an unspecified mean sea level",
        }
    }
}

impl VerticalDatum {
    /// `VerticalCSTypeGeoKey` の値から判定する。
    ///
    /// キーが無い場合は [`VerticalDatum::Unspecified`]。
    #[must_use]
    pub fn from_geo_key(value: Option<u16>) -> Self {
        match value {
            None => Self::Unspecified,
            Some(EPSG_WGS84_ELLIPSOIDAL_3D | EPSG_WGS84_ELLIPSOID_HEIGHT) => Self::Ellipsoidal,
            Some(EPSG_EGM2008_HEIGHT) => Self::Geoid(GeoidModel::Egm2008),
            Some(EPSG_EGM96_HEIGHT) => Self::Geoid(GeoidModel::Egm96),
            Some(EPSG_MEAN_SEA_LEVEL) => Self::Geoid(GeoidModel::UnspecifiedMeanSeaLevel),
            Some(code) => Self::Unsupported(code),
        }
    }

    /// そのまま楕円体高として使えるか。
    #[must_use]
    pub const fn is_ellipsoidal(self) -> bool {
        matches!(self, Self::Ellipsoidal)
    }

    /// 高さをそのまま使ったときに入る誤差の説明。
    ///
    /// **数値は出さない。** ジオイド高は場所によって −107〜+86 m と大きく
    /// 変わるので、代表値を書くとかえって誤解を招く。
    #[must_use]
    pub fn mismatch_reason(self) -> Option<String> {
        match self {
            Self::Ellipsoidal => None,
            Self::Geoid(model) => Some(format!(
                "the heights are orthometric, referenced to {}. Using them as WGS84 \
                 ellipsoidal heights adds the geoid undulation as a systematic error \
                 (-107 m to +86 m worldwide, about +30 to +40 m over Japan)",
                model.name()
            )),
            Self::Unspecified => Some(
                "the GeoTIFF does not record a vertical datum. Assuming ellipsoidal heights \
                 is not safe: DEMs that omit the datum are usually orthometric"
                    .to_owned(),
            ),
            Self::Unsupported(code) => Some(format!(
                "EPSG:{code} is not a vertical datum this project knows how to handle"
            )),
        }
    }
}

impl fmt::Display for VerticalDatum {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ellipsoidal => write!(formatter, "WGS84 ellipsoidal height"),
            Self::Geoid(model) => write!(formatter, "orthometric height above {}", model.name()),
            Self::Unspecified => write!(formatter, "unspecified"),
            Self::Unsupported(code) => write!(formatter, "EPSG:{code} (unsupported)"),
        }
    }
}

/// 鉛直基準が契約と合わないまま焼こうとしたときの拒否。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerticalDatumMismatch {
    /// 読み取った基準。
    pub datum: VerticalDatum,
}

impl fmt::Display for VerticalDatumMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = self
            .datum
            .mismatch_reason()
            .unwrap_or_else(|| "the vertical datum does not match".to_owned());
        write!(
            formatter,
            "the source DEM is not in WGS84 ellipsoidal heights: {reason}.\n\
             \n\
             `.fsdem` stores WGS84 ellipsoidal heights, because `Geodetic::altitude` \
             is defined that way (ADR-0002).\n\
             \n\
             There is no geoid model in this build, so the conversion cannot be done here. \
             Either supply a DEM already in ellipsoidal heights, or pass \
             `--assume-ellipsoidal` to bake the values unchanged and accept the \
             systematic error."
        )
    }
}

impl std::error::Error for VerticalDatumMismatch {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wgs84_codes_are_recognised_as_ellipsoidal() {
        // EPSG:4979 と 5030 はどちらも WGS84 楕円体高。
        for code in [EPSG_WGS84_ELLIPSOIDAL_3D, EPSG_WGS84_ELLIPSOID_HEIGHT] {
            let datum = VerticalDatum::from_geo_key(Some(code));
            assert!(
                datum.is_ellipsoidal(),
                "EPSG:{code} should be ellipsoidal, got {datum}"
            );
            assert!(datum.mismatch_reason().is_none());
        }
    }

    #[test]
    fn copernicus_dem_is_recognised_as_egm2008() {
        // **これが本題。** Copernicus DEM GLO-30 は EPSG:3855。
        let datum = VerticalDatum::from_geo_key(Some(EPSG_EGM2008_HEIGHT));
        assert_eq!(datum, VerticalDatum::Geoid(GeoidModel::Egm2008));
        assert!(
            !datum.is_ellipsoidal(),
            "EGM2008 heights must not pass as ellipsoidal"
        );
    }

    #[test]
    fn egm96_and_bare_mean_sea_level_are_both_geoid_referenced() {
        assert_eq!(
            VerticalDatum::from_geo_key(Some(EPSG_EGM96_HEIGHT)),
            VerticalDatum::Geoid(GeoidModel::Egm96)
        );
        assert_eq!(
            VerticalDatum::from_geo_key(Some(EPSG_MEAN_SEA_LEVEL)),
            VerticalDatum::Geoid(GeoidModel::UnspecifiedMeanSeaLevel)
        );
    }

    #[test]
    fn a_missing_key_is_unspecified_not_assumed_ellipsoidal() {
        // **ここが肝。** 「書いていない = 楕円体高」と決めつけると、
        // 実務でいちばん多い誤りをそのまま通す。
        let datum = VerticalDatum::from_geo_key(None);
        assert_eq!(datum, VerticalDatum::Unspecified);
        assert!(
            !datum.is_ellipsoidal(),
            "a missing vertical datum must not pass as ellipsoidal"
        );
        assert!(datum.mismatch_reason().is_some());
    }

    #[test]
    fn an_unknown_code_is_reported_with_its_number() {
        // 番号を出さないと、利用者が調べようがない。
        let datum = VerticalDatum::from_geo_key(Some(1234));
        assert_eq!(datum, VerticalDatum::Unsupported(1234));
        let reason = datum.mismatch_reason().expect("unsupported has a reason");
        assert!(reason.contains("1234"), "{reason}");
    }

    #[test]
    fn every_mismatch_explains_what_to_do() {
        // 「合いません」だけでは、利用者は次の手を打てない。
        for datum in [
            VerticalDatum::Geoid(GeoidModel::Egm2008),
            VerticalDatum::Unspecified,
            VerticalDatum::Unsupported(9999),
        ] {
            let message = VerticalDatumMismatch { datum }.to_string();
            assert!(
                message.contains("--assume-ellipsoidal"),
                "the escape hatch should be named: {message}"
            );
            assert!(
                message.contains("ADR-0002"),
                "the contract should be cited: {message}"
            );
        }
    }

    #[test]
    fn the_geoid_message_does_not_quote_a_single_undulation_value() {
        // ジオイド高は場所で -107..+86 m と変わる。代表値 1 つを書くと
        // 「その値を足せばよい」と誤解される。
        let reason = VerticalDatum::Geoid(GeoidModel::Egm2008)
            .mismatch_reason()
            .expect("a geoid datum has a reason");
        assert!(
            reason.contains("-107") && reason.contains("+86"),
            "the range should be given, not a single value: {reason}"
        );
    }

    #[test]
    fn displaying_a_datum_names_the_model() {
        assert_eq!(
            VerticalDatum::Ellipsoidal.to_string(),
            "WGS84 ellipsoidal height"
        );
        assert!(
            VerticalDatum::Geoid(GeoidModel::Egm2008)
                .to_string()
                .contains("EGM2008")
        );
        assert!(
            VerticalDatum::Unsupported(42)
                .to_string()
                .contains("EPSG:42")
        );
    }

    #[test]
    fn every_message_is_ascii() {
        // CLI の出力。端末やログの文字コードに依存させない。
        for datum in [
            VerticalDatum::Ellipsoidal,
            VerticalDatum::Geoid(GeoidModel::Egm2008),
            VerticalDatum::Geoid(GeoidModel::Egm96),
            VerticalDatum::Geoid(GeoidModel::UnspecifiedMeanSeaLevel),
            VerticalDatum::Unspecified,
            VerticalDatum::Unsupported(7),
        ] {
            assert!(datum.to_string().is_ascii(), "{datum}");
            if let Some(reason) = datum.mismatch_reason() {
                assert!(reason.is_ascii(), "{reason}");
            }
            assert!(VerticalDatumMismatch { datum }.to_string().is_ascii());
        }
    }
}
