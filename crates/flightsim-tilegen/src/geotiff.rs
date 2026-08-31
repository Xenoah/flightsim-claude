//! GeoTIFF の読み込みと地理参照。
//!
//! # ここは「オフライン側」
//!
//! 実行時にこのコードが動くことはない。GeoTIFF のデコードはフレーム予算に
//! 収まらないため、焼き込み時にのみ使う（ADR-0003）。
//!
//! # 対応する範囲
//!
//! Copernicus DEM GLO-30 が該当する形、すなわち **EPSG:4326（地理座標系）の
//! 単バンド浮動小数点ラスタ**のみを読む。投影座標系のラスタを度として読むと
//! 静かに全く違う場所の地形になるため、`GTModelTypeGeoKey` を検査して弾く。

use crate::vertical_datum::{VERTICAL_CS_TYPE_GEO_KEY, VerticalDatum};
use core::f64::consts::{PI, TAU};
use flightsim_core::{Degrees, Geodetic, Meters, Radians};
use std::path::{Path, PathBuf};
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

/// GeoTIFF の読み込みで起きるエラー。
#[derive(Debug)]
pub enum RasterError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Tiff {
        path: PathBuf,
        source: tiff::TiffError,
    },
    /// 地理参照タグが無い。ただの TIFF であって GeoTIFF ではない。
    MissingGeoreference { path: PathBuf, tag: &'static str },
    /// 地理座標系ではない（投影座標系など）。
    NotGeographic { path: PathBuf, model_type: u16 },
    /// 単バンド浮動小数点以外。
    UnsupportedSampleFormat { path: PathBuf, description: String },
    /// 画素サイズが 0 または負、寸法が 2 未満など。
    DegenerateGeometry { path: PathBuf, reason: String },
}

impl core::fmt::Display for RasterError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to open {}: {source}", path.display())
            }
            Self::Tiff { path, source } => {
                write!(formatter, "failed to decode {}: {source}", path.display())
            }
            Self::MissingGeoreference { path, tag } => write!(
                formatter,
                "{} has no {tag}; it is a plain TIFF, not a GeoTIFF",
                path.display()
            ),
            Self::NotGeographic { path, model_type } => write!(
                formatter,
                "{} uses GTModelTypeGeoKey {model_type}; only geographic (EPSG:4326, key value 2) \
                 rasters are supported. Reading a projected raster as degrees would silently \
                 place the terrain somewhere else entirely.",
                path.display()
            ),
            Self::UnsupportedSampleFormat { path, description } => write!(
                formatter,
                "{} has an unsupported sample layout ({description}); \
                 expected a single-band floating point raster",
                path.display()
            ),
            Self::DegenerateGeometry { path, reason } => {
                write!(
                    formatter,
                    "{} has invalid geometry: {reason}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for RasterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Tiff { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// GeoTIFF の画素配置。
///
/// **半画素ずれる。** `PixelIsArea` では基準点が画素の外角、`PixelIsPoint` では
/// 画素中心を指す。取り違えると地形が 30 m 級のデータで 15 m ずれる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RasterPixelConvention {
    /// `GTRasterTypeGeoKey = 1`。GeoTIFF の既定。
    Area,
    /// `GTRasterTypeGeoKey = 2`。
    Point,
}

/// 地理参照された標高ラスタ。
///
/// 格子は行優先で、**先頭行が最北端**（`HeightGrid` と同じ向き）。
#[derive(Debug, Clone)]
pub struct GeoRaster {
    width: u32,
    height: u32,
    samples: Vec<f32>,
    /// 画素 (0, 0) の**中心**の経度。
    origin_longitude: f64,
    /// 画素 (0, 0) の**中心**の緯度。
    origin_latitude: f64,
    /// 東向き 1 画素あたりの経度差（正）。
    pixel_longitude: f64,
    /// 南向き 1 画素あたりの緯度差（正）。
    pixel_latitude: f64,
    nodata: Option<f32>,
    /// 高さが何を基準にしているか。
    ///
    /// **読み取るだけで、ここでは変換しない。** 変換にはジオイドモデルが
    /// 要る（[`crate::vertical_datum`]）。
    vertical_datum: VerticalDatum,
}

impl GeoRaster {
    /// GeoTIFF ファイルを読む。
    ///
    /// # Errors
    ///
    /// ファイルが開けない、TIFF として壊れている、地理参照タグが無い、
    /// 地理座標系でない、単バンド浮動小数点でない、幾何が縮退している場合。
    pub fn open(path: &Path) -> Result<Self, RasterError> {
        let file = std::fs::File::open(path).map_err(|source| RasterError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::decode(std::io::BufReader::new(file), path)
    }

    /// 任意の `Read + Seek` から読む。テストが合成 GeoTIFF を渡すために公開している。
    ///
    /// # Errors
    ///
    /// [`Self::open`] と同じ。
    pub fn decode<R: std::io::Read + std::io::Seek>(
        reader: R,
        path: &Path,
    ) -> Result<Self, RasterError> {
        let tiff_error = |source| RasterError::Tiff {
            path: path.to_path_buf(),
            source,
        };

        let mut decoder = Decoder::new(reader).map_err(tiff_error)?;
        let (width, height) = decoder.dimensions().map_err(tiff_error)?;

        if width < 2 || height < 2 {
            return Err(RasterError::DegenerateGeometry {
                path: path.to_path_buf(),
                reason: format!("raster is {width}×{height}; need at least 2×2 to interpolate"),
            });
        }

        // --- 地理参照 ---

        let scale = decoder
            .get_tag_f64_vec(Tag::ModelPixelScaleTag)
            .map_err(|_| RasterError::MissingGeoreference {
                path: path.to_path_buf(),
                tag: "ModelPixelScaleTag (33550)",
            })?;
        let tiepoint = decoder
            .get_tag_f64_vec(Tag::ModelTiepointTag)
            .map_err(|_| RasterError::MissingGeoreference {
                path: path.to_path_buf(),
                tag: "ModelTiepointTag (33922)",
            })?;

        if scale.len() < 2 || tiepoint.len() < 6 {
            return Err(RasterError::DegenerateGeometry {
                path: path.to_path_buf(),
                reason: format!(
                    "ModelPixelScale has {} values and ModelTiepoint has {}; expected >= 2 and >= 6",
                    scale.len(),
                    tiepoint.len()
                ),
            });
        }

        // GeoTIFF は投影座標系も表せる。度として読むと全く違う場所になるので検査する。
        let geo_keys = decoder
            .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
            .unwrap_or_default();
        match geo_key(&geo_keys, GT_MODEL_TYPE_GEO_KEY) {
            Some(MODEL_TYPE_GEOGRAPHIC) | None => {}
            Some(model_type) => {
                return Err(RasterError::NotGeographic {
                    path: path.to_path_buf(),
                    model_type,
                });
            }
        }

        // 鉛直基準。**ここでは弾かない。** 焼くかどうかは呼び出し側の判断で、
        // 読むこと自体は妨げない（検査や比較に使えるため）。
        let vertical_datum =
            VerticalDatum::from_geo_key(geo_key(&geo_keys, VERTICAL_CS_TYPE_GEO_KEY));

        let convention = match geo_key(&geo_keys, GT_RASTER_TYPE_GEO_KEY) {
            Some(2) => RasterPixelConvention::Point,
            // GeoTIFF の既定は PixelIsArea。キーが無い場合もこちら。
            _ => RasterPixelConvention::Area,
        };

        let (pixel_longitude, pixel_latitude) = (scale[0], scale[1]);
        if !(pixel_longitude.is_finite() && pixel_latitude.is_finite())
            || pixel_longitude <= 0.0
            || pixel_latitude <= 0.0
        {
            return Err(RasterError::DegenerateGeometry {
                path: path.to_path_buf(),
                reason: format!(
                    "ModelPixelScale is ({pixel_longitude}, {pixel_latitude}); both must be positive and finite"
                ),
            });
        }

        // ModelTiepoint は [i, j, k, x, y, z]。ラスタ点 (i, j) が地理座標 (x, y) に対応する。
        let (raster_i, raster_j) = (tiepoint[0], tiepoint[1]);
        let (tie_longitude, tie_latitude) = (tiepoint[3], tiepoint[4]);
        if ![raster_i, raster_j, tie_longitude, tie_latitude]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(RasterError::DegenerateGeometry {
                path: path.to_path_buf(),
                reason: "ModelTiepoint contains non-finite values".to_owned(),
            });
        }

        // 基準点を画素 (0, 0) の中心へ移す。
        // PixelIsArea では基準点が画素の外角なので、半画素ぶん内側へずらす。
        let half = match convention {
            RasterPixelConvention::Area => 0.5,
            RasterPixelConvention::Point => 0.0,
        };
        let origin_longitude_degrees = tie_longitude - (raster_i - half) * pixel_longitude;
        let origin_latitude_degrees = tie_latitude + (raster_j - half) * pixel_latitude;

        // --- 画素 ---

        let samples = match decoder.read_image().map_err(tiff_error)? {
            DecodingResult::F32(values) => values,
            #[allow(
                clippy::cast_precision_loss,
                reason = "整数 DEM の標高は ±9000 m 程度。f32 で表現できる"
            )]
            DecodingResult::I16(values) => values.into_iter().map(f32::from).collect(),
            #[allow(
                clippy::cast_precision_loss,
                reason = "整数 DEM の標高は ±9000 m 程度。f32 で表現できる"
            )]
            DecodingResult::U16(values) => values.into_iter().map(f32::from).collect(),
            other => {
                return Err(RasterError::UnsupportedSampleFormat {
                    path: path.to_path_buf(),
                    description: format!("{:?} samples", core::mem::discriminant(&other)),
                });
            }
        };

        let expected = (width as usize) * (height as usize);
        if samples.len() != expected {
            return Err(RasterError::UnsupportedSampleFormat {
                path: path.to_path_buf(),
                description: format!(
                    "{} samples for a {width}×{height} raster (expected {expected}); \
                     multi-band rasters are not supported",
                    samples.len()
                ),
            });
        }

        let nodata = decoder
            .get_tag_ascii_string(Tag::GdalNodata)
            .ok()
            .and_then(|text| text.trim().trim_end_matches('\0').parse::<f32>().ok());

        Ok(Self {
            width,
            height,
            samples,
            origin_longitude: Degrees(origin_longitude_degrees).to_radians().get(),
            origin_latitude: Degrees(origin_latitude_degrees).to_radians().get(),
            pixel_longitude: Degrees(pixel_longitude).to_radians().get(),
            pixel_latitude: Degrees(pixel_latitude).to_radians().get(),
            nodata,
            vertical_datum,
        })
    }

    /// 高さが何を基準にしているか。
    ///
    /// **そのまま楕円体高として使ってよいのは
    /// [`VerticalDatum::Ellipsoidal`] のときだけ。**
    #[must_use]
    pub const fn vertical_datum(&self) -> VerticalDatum {
        self.vertical_datum
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// 東向き 1 画素あたりの経度差。
    #[must_use]
    pub const fn pixel_longitude(&self) -> Radians {
        Radians(self.pixel_longitude)
    }

    /// 南向き 1 画素あたりの緯度差。
    #[must_use]
    pub const fn pixel_latitude(&self) -> Radians {
        Radians(self.pixel_latitude)
    }

    /// ラスタが覆う範囲（画素の外縁まで）。
    #[must_use]
    pub fn coverage(&self) -> RasterCoverage {
        RasterCoverage {
            west: Radians(self.origin_longitude - self.pixel_longitude * 0.5),
            east: Radians(
                self.origin_longitude + (f64::from(self.width) - 0.5) * self.pixel_longitude,
            ),
            north: Radians(self.origin_latitude + self.pixel_latitude * 0.5),
            south: Radians(
                self.origin_latitude - (f64::from(self.height) - 0.5) * self.pixel_latitude,
            ),
        }
    }

    /// 格子点の値。nodata と範囲外は `None`。
    ///
    /// 添字は `i64` で受ける。バイリニア補間が `column + 1` や負の位置を素直に
    /// 試せるようにするため。範囲外はここで `None` に落ちる。
    fn pixel(&self, column: i64, row: i64) -> Option<f32> {
        // try_from が負を弾き、比較が上限を弾く。キャストを挟まないので
        // 32bit 環境での切り詰めも起こり得ない。
        let column = u32::try_from(column).ok()?;
        let row = u32::try_from(row).ok()?;
        if column >= self.width || row >= self.height {
            return None;
        }

        let index = (row as usize) * (self.width as usize) + (column as usize);
        let value = self.samples[index];

        if !value.is_finite() {
            return None;
        }
        match self.nodata {
            // nodata は「等しい値」で指定される。ここは意図的な等値比較。
            #[allow(
                clippy::float_cmp,
                reason = "GDAL_NODATA は特定のビットパターンとの一致で判定する仕様"
            )]
            Some(sentinel) if value == sentinel => None,
            _ => Some(value),
        }
    }

    /// 指定した足跡（footprint）に対する標高。
    ///
    /// 足跡が元画素より粗い場合は**面積平均**、細かい場合はバイリニア補間を使う。
    ///
    /// 粗い側で単純な点サンプリングを使うと、30 m の元データから粗いタイルを
    /// 焼いた際にエイリアシングノイズが乗る。そのノイズがそのまま幾何誤差として
    /// 算出され、平野が過剰に細分化される。
    ///
    /// 覆う画素が全て nodata / 範囲外なら `None`。
    #[must_use]
    pub fn sample(&self, position: Geodetic, footprint: (Radians, Radians)) -> Option<Meters> {
        let longitude = position.longitude.get();
        let latitude = position.latitude.get();

        let spans_multiple_pixels =
            footprint.0.get() > self.pixel_longitude || footprint.1.get() > self.pixel_latitude;

        let value = if spans_multiple_pixels {
            self.area_average(longitude, latitude, footprint)
        } else {
            self.bilinear(longitude, latitude)
        }?;

        Some(Meters(f64::from(value)))
    }

    /// 経度差を `[-π, π)` に畳んだうえで画素座標へ写す。
    ///
    /// 単純な引き算だと、日付変更線をまたぐ位置（+180° と -180° は同じ場所）で
    /// 地球一周ぶんの差が出て、範囲外と判定されてしまう。
    fn column_of(&self, longitude: f64) -> f64 {
        let delta = (longitude - self.origin_longitude + PI).rem_euclid(TAU) - PI;
        delta / self.pixel_longitude
    }

    fn bilinear(&self, longitude: f64, latitude: f64) -> Option<f32> {
        let x = self.column_of(longitude);
        let y = (self.origin_latitude - latitude) / self.pixel_latitude;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }

        let (column, row) = (x.floor(), y.floor());
        #[allow(
            clippy::cast_possible_truncation,
            reason = "有限性は直前に検査済み。範囲外は pixel() が None を返す"
        )]
        let (column, row) = (column as i64, row as i64);
        let (fraction_x, fraction_y) = (x - x.floor(), y - y.floor());

        // nodata の角は重みから外し、残りで正規化する。
        // 「1 つでも nodata なら全体を捨てる」だと海岸線が虫食いになる。
        let corners = [
            (
                self.pixel(column, row),
                (1.0 - fraction_x) * (1.0 - fraction_y),
            ),
            (self.pixel(column + 1, row), fraction_x * (1.0 - fraction_y)),
            (self.pixel(column, row + 1), (1.0 - fraction_x) * fraction_y),
            (self.pixel(column + 1, row + 1), fraction_x * fraction_y),
        ];

        let mut total = 0.0_f64;
        let mut weight_sum = 0.0_f64;
        for (value, weight) in corners {
            if let Some(value) = value {
                total += f64::from(value) * weight;
                weight_sum += weight;
            }
        }

        if weight_sum > 0.0 {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "標高は ±9000 m の範囲。f32 の分解能は約 0.001 m で十分"
            )]
            let averaged = (total / weight_sum) as f32;
            return Some(averaged);
        }

        // 重みを持つ角が全て nodata だった場合（格子点にちょうど乗ったときなど）は、
        // 重みゼロの角からでも拾う。ここで諦めるとピンホールが開く。
        corners.into_iter().find_map(|(value, _)| value)
    }

    fn area_average(
        &self,
        longitude: f64,
        latitude: f64,
        footprint: (Radians, Radians),
    ) -> Option<f32> {
        let half_longitude = footprint.0.get() * 0.5;
        let half_latitude = footprint.1.get() * 0.5;

        let centre_column = self.column_of(longitude);
        let x_start = centre_column - half_longitude / self.pixel_longitude;
        let x_end = centre_column + half_longitude / self.pixel_longitude;
        let y_start = (self.origin_latitude - latitude - half_latitude) / self.pixel_latitude;
        let y_end = (self.origin_latitude - latitude + half_latitude) / self.pixel_latitude;

        if ![x_start, x_end, y_start, y_end]
            .iter()
            .all(|value| value.is_finite())
        {
            return None;
        }

        #[allow(
            clippy::cast_possible_truncation,
            reason = "有限性は直前に検査済み。範囲外は pixel() が None を返す"
        )]
        let (first_column, last_column) = (x_start.round() as i64, x_end.round() as i64);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "有限性は直前に検査済み。範囲外は pixel() が None を返す"
        )]
        let (first_row, last_row) = (y_start.round() as i64, y_end.round() as i64);

        let mut total = 0.0_f64;
        let mut count = 0_u32;
        for row in first_row..=last_row {
            for column in first_column..=last_column {
                if let Some(value) = self.pixel(column, row) {
                    total += f64::from(value);
                    count += 1;
                }
            }
        }

        if count == 0 {
            // 足跡が元画素より粗くても、端では 1 画素も拾えないことがある。
            // 中心のバイリニアへ落として虫食いを避ける。
            return self.bilinear(longitude, latitude);
        }

        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_lossless,
            reason = "標高は ±9000 m の範囲。f32 の分解能は約 0.001 m で十分"
        )]
        let averaged = (total / f64::from(count)) as f32;
        Some(averaged)
    }
}

/// ラスタが覆う地理的範囲（画素の外縁まで）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterCoverage {
    pub west: Radians,
    pub south: Radians,
    pub east: Radians,
    pub north: Radians,
}

// GeoTIFF の GeoKey 番号。
const GT_MODEL_TYPE_GEO_KEY: u16 = 1024;
const GT_RASTER_TYPE_GEO_KEY: u16 = 1025;
const MODEL_TYPE_GEOGRAPHIC: u16 = 2;

/// `GeoKeyDirectoryTag` から短整数キーを引く。
///
/// レイアウトは先頭 4 語がヘッダ（版・改訂・小改訂・キー数）で、以降 4 語ずつが
/// `[KeyID, TIFFTagLocation, Count, Value]`。`TIFFTagLocation == 0` のときだけ
/// 最後の語が値そのもの（それ以外は別タグへの参照で、ここでは扱わない）。
fn geo_key(keys: &[u16], key_id: u16) -> Option<u16> {
    let count = *keys.get(3)? as usize;
    (0..count).find_map(|index| {
        let base = 4 + index * 4;
        let entry = keys.get(base..base + 4)?;
        (entry[0] == key_id && entry[1] == 0).then_some(entry[3])
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "テスト用の合成ラスタ生成。f32 の精度で十分"
    )]

    use super::*;
    use crate::testing::{GeoTiffBuilder, PixelConvention};

    fn ramp(width: u32, height: u32) -> Vec<f32> {
        (0..height)
            .flat_map(|row| (0..width).map(move |column| (column * 10 + row) as f32))
            .collect()
    }

    #[test]
    fn a_synthetic_geotiff_round_trips_its_georeference() {
        let bytes = GeoTiffBuilder::new(4, 3, ramp(4, 3))
            .origin(139.0, 36.0)
            .pixel_size(0.25, 0.5)
            .build();

        let raster = GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<memory>"))
            .expect("the synthetic file should decode");

        assert_eq!((raster.width(), raster.height()), (4, 3));
        assert!((raster.pixel_longitude().to_degrees().get() - 0.25).abs() < 1e-12);
        assert!((raster.pixel_latitude().to_degrees().get() - 0.5).abs() < 1e-12);

        let coverage = raster.coverage();
        assert!((coverage.west.to_degrees().get() - 139.0).abs() < 1e-9);
        assert!((coverage.north.to_degrees().get() - 36.0).abs() < 1e-9);
        assert!((coverage.east.to_degrees().get() - 140.0).abs() < 1e-9);
        assert!((coverage.south.to_degrees().get() - 34.5).abs() < 1e-9);
    }

    #[test]
    fn pixel_centres_sample_their_own_value() {
        let bytes = GeoTiffBuilder::new(4, 3, ramp(4, 3))
            .origin(139.0, 36.0)
            .pixel_size(0.25, 0.5)
            .build();
        let raster =
            GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<memory>")).expect("decode");

        // 足跡を 1 画素より小さくしてバイリニア経路を通す。
        let footprint = (Degrees(0.01).to_radians(), Degrees(0.01).to_radians());

        for row in 0..3_u32 {
            for column in 0..4_u32 {
                let longitude = 139.0 + (f64::from(column) + 0.5) * 0.25;
                let latitude = 36.0 - (f64::from(row) + 0.5) * 0.5;
                let sampled = raster
                    .sample(Geodetic::from_degrees(latitude, longitude, 0.0), footprint)
                    .expect("inside the raster");
                let expected = f64::from(column * 10 + row);
                assert!(
                    (sampled.get() - expected).abs() < 1e-4,
                    "pixel ({column}, {row}) sampled as {sampled} m, expected {expected} m"
                );
            }
        }
    }

    #[test]
    fn pixel_is_point_shifts_the_georeference_by_half_a_pixel() {
        // 取り違えると 30 m データで 15 m ずれる。座標系の定番の事故。
        let area = GeoTiffBuilder::new(4, 3, ramp(4, 3))
            .origin(139.0, 36.0)
            .pixel_size(0.25, 0.5)
            .convention(PixelConvention::Area)
            .build();
        let point = GeoTiffBuilder::new(4, 3, ramp(4, 3))
            .origin(139.0, 36.0)
            .pixel_size(0.25, 0.5)
            .convention(PixelConvention::Point)
            .build();

        let area =
            GeoRaster::decode(std::io::Cursor::new(area), Path::new("<area>")).expect("area");
        let point =
            GeoRaster::decode(std::io::Cursor::new(point), Path::new("<point>")).expect("point");

        let offset =
            point.coverage().west.to_degrees().get() - area.coverage().west.to_degrees().get();
        assert!(
            (offset + 0.125).abs() < 1e-9,
            "PixelIsPoint should shift the western edge by half a pixel, got {offset}°"
        );
    }

    #[test]
    fn nodata_pixels_are_excluded() {
        let mut samples = vec![100.0_f32; 16];
        samples[5] = -9999.0;
        let bytes = GeoTiffBuilder::new(4, 4, samples)
            .origin(0.0, 0.0)
            .pixel_size(1.0, 1.0)
            .nodata(-9999.0)
            .build();
        let raster =
            GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<memory>")).expect("decode");

        // nodata 画素の中心でも、周囲の有効画素から値が復元される
        // （角を捨てて残りで正規化しているため）。
        let sampled = raster.sample(
            Geodetic::from_degrees(-1.5, 1.5, 0.0),
            (Degrees(0.01).to_radians(), Degrees(0.01).to_radians()),
        );
        assert!(
            sampled.is_some(),
            "a single nodata pixel should not create a hole"
        );
        assert!((sampled.expect("some").get() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn a_fully_nodata_footprint_reports_no_coverage() {
        let bytes = GeoTiffBuilder::new(4, 4, vec![-9999.0_f32; 16])
            .origin(0.0, 0.0)
            .pixel_size(1.0, 1.0)
            .nodata(-9999.0)
            .build();
        let raster =
            GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<memory>")).expect("decode");

        assert!(
            raster
                .sample(
                    Geodetic::from_degrees(-2.0, 2.0, 0.0),
                    (Degrees(0.01).to_radians(), Degrees(0.01).to_radians())
                )
                .is_none()
        );
    }

    #[test]
    fn positions_outside_the_raster_report_no_coverage() {
        let bytes = GeoTiffBuilder::new(4, 4, vec![50.0_f32; 16])
            .origin(0.0, 0.0)
            .pixel_size(1.0, 1.0)
            .build();
        let raster =
            GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<memory>")).expect("decode");

        let footprint = (Degrees(0.01).to_radians(), Degrees(0.01).to_radians());
        assert!(
            raster
                .sample(Geodetic::from_degrees(-50.0, 50.0, 0.0), footprint)
                .is_none()
        );
        assert!(
            raster
                .sample(Geodetic::from_degrees(-2.0, 2.0, 0.0), footprint)
                .is_some()
        );
    }

    #[test]
    fn a_coarse_footprint_averages_instead_of_point_sampling() {
        // 点サンプリングだと粗いタイルでエイリアシングが乗り、それが幾何誤差として
        // 算出されて平野が過剰に細分化される。
        let mut samples = vec![0.0_f32; 64];
        for (index, sample) in samples.iter_mut().enumerate() {
            // 1 画素おきに 0 / 1000 が交互に並ぶ高周波地形。平均は 500。
            *sample = if index % 2 == 0 { 0.0 } else { 1_000.0 };
        }
        let bytes = GeoTiffBuilder::new(8, 8, samples)
            .origin(0.0, 0.0)
            .pixel_size(1.0, 1.0)
            .build();
        let raster =
            GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<memory>")).expect("decode");

        // 4 画素ぶんの足跡で拾う。
        let sampled = raster
            .sample(
                Geodetic::from_degrees(-4.0, 4.0, 0.0),
                (Degrees(4.0).to_radians(), Degrees(4.0).to_radians()),
            )
            .expect("inside the raster");

        // 点サンプリングなら 0 か 1000 のどちらかに張り付く。平均されていれば中間に来る。
        assert!(
            (300.0..=700.0).contains(&sampled.get()),
            "a coarse footprint sampled {sampled} m; that looks like point sampling, \
             not area averaging (which should land near 500 m)"
        );
    }

    #[test]
    fn a_projected_raster_is_rejected_rather_than_read_as_degrees() {
        // 投影座標系を度として読むと、静かに全く違う場所の地形になる。
        let bytes = GeoTiffBuilder::new(4, 4, vec![10.0_f32; 16])
            .origin(500_000.0, 4_000_000.0)
            .pixel_size(30.0, 30.0)
            .model_type(1) // ModelTypeProjected
            .build();

        match GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<utm>")) {
            Err(RasterError::NotGeographic { model_type: 1, .. }) => {}
            other => panic!("a projected raster should be rejected, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_tiff_without_georeference_is_rejected() {
        let bytes = GeoTiffBuilder::new(4, 4, vec![10.0_f32; 16])
            .without_georeference()
            .build();

        assert!(matches!(
            GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<plain>")),
            Err(RasterError::MissingGeoreference { .. })
        ));
    }

    #[test]
    fn a_zero_pixel_size_is_rejected() {
        let bytes = GeoTiffBuilder::new(4, 4, vec![10.0_f32; 16])
            .origin(0.0, 0.0)
            .pixel_size(0.0, 1.0)
            .build();

        assert!(matches!(
            GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<degenerate>")),
            Err(RasterError::DegenerateGeometry { .. })
        ));
    }

    #[test]
    fn garbage_input_is_reported_rather_than_panicking() {
        for bytes in [
            vec![],
            b"not a tiff at all".to_vec(),
            b"II\x2a\x00\xff\xff\xff\xff".to_vec(),
        ] {
            let result = GeoRaster::decode(std::io::Cursor::new(bytes), Path::new("<garbage>"));
            assert!(result.is_err());
        }
    }

    // --- GeoKey の解析 ---

    #[test]
    fn geo_keys_are_looked_up_by_id() {
        // ヘッダ 4 語 + キー 2 個。
        let keys = [1, 1, 0, 2, 1024, 0, 1, 2, 1025, 0, 1, 1];
        assert_eq!(geo_key(&keys, 1024), Some(2));
        assert_eq!(geo_key(&keys, 1025), Some(1));
        assert_eq!(geo_key(&keys, 2048), None);
    }

    #[test]
    fn a_truncated_geo_key_directory_does_not_panic() {
        assert_eq!(geo_key(&[], 1024), None);
        assert_eq!(geo_key(&[1, 1, 0], 1024), None);
        // キー数が 5 と宣言されているが実体が 1 つ分しかない。
        assert_eq!(geo_key(&[1, 1, 0, 5, 1024, 0, 1, 2], 1024), Some(2));
        assert_eq!(geo_key(&[1, 1, 0, 5, 1024, 0, 1], 1024), None);
    }

    #[test]
    fn geo_keys_stored_indirectly_are_ignored_rather_than_misread() {
        // TIFFTagLocation != 0 は別タグへの参照。値そのものではない。
        let keys = [1, 1, 0, 1, 1024, 34_736, 1, 0];
        assert_eq!(geo_key(&keys, 1024), None);
    }
}
