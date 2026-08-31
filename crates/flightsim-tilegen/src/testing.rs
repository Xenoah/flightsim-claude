//! 合成 GeoTIFF の組み立て。
//!
//! # なぜバイト列を手で組むのか
//!
//! CI に実データを置けない（Copernicus DEM は全球で数百 GB）。かといって
//! 地理参照の解釈をモックで置き換えると、**実際のデコード経路が一度も検査されない**。
//!
//! そこで、非圧縮・単ストリップ・単バンド f32 という最小構成の TIFF を
//! バイト単位で組み立てる。エンコーダのクレート API に依存しないので、
//! ヘッダの中身を狙って壊すテスト（投影座標系、地理参照なし、画素サイズ 0）も書ける。
//!
//! テスト専用だが `#[cfg(test)]` にはしていない。統合テストからも使うため。

#![allow(
    clippy::cast_possible_truncation,
    reason = "TIFF のオフセットとカウントは仕様上 u32。テスト用ラスタは高々数 KB"
)]

/// GeoTIFF の画素配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelConvention {
    /// `GTRasterTypeGeoKey = 1`。基準点は画素の外角。GeoTIFF の既定。
    Area,
    /// `GTRasterTypeGeoKey = 2`。基準点は画素の中心。
    Point,
}

/// 最小構成の GeoTIFF をバイト列として組み立てる。
#[derive(Debug, Clone)]
pub struct GeoTiffBuilder {
    width: u32,
    height: u32,
    samples: Vec<f32>,
    /// 基準点の (経度, 緯度) `度`。[`PixelConvention`] により画素の角か中心かが変わる。
    origin: (f64, f64),
    /// 1 画素あたりの (経度差, 緯度差) `度`。
    pixel_size: (f64, f64),
    convention: PixelConvention,
    model_type: u16,
    /// `VerticalCSTypeGeoKey`（4096）。`None` なら書かない。
    vertical_cs_type: Option<u16>,
    nodata: Option<f32>,
    georeferenced: bool,
}

impl GeoTiffBuilder {
    /// # Panics
    ///
    /// サンプル数が `width * height` と一致しない場合。
    #[must_use]
    pub fn new(width: u32, height: u32, samples: Vec<f32>) -> Self {
        assert_eq!(
            samples.len(),
            (width as usize) * (height as usize),
            "sample count must match the raster dimensions"
        );
        Self {
            width,
            height,
            samples,
            origin: (0.0, 0.0),
            pixel_size: (1.0, 1.0),
            convention: PixelConvention::Area,
            // 2 = ModelTypeGeographic (EPSG:4326)
            model_type: 2,
            // 既定では書かない。**実務の DEM も書いていないことが多い。**
            vertical_cs_type: None,
            nodata: None,
            georeferenced: true,
        }
    }

    #[must_use]
    pub const fn origin(mut self, longitude_degrees: f64, latitude_degrees: f64) -> Self {
        self.origin = (longitude_degrees, latitude_degrees);
        self
    }

    #[must_use]
    pub const fn pixel_size(mut self, longitude_degrees: f64, latitude_degrees: f64) -> Self {
        self.pixel_size = (longitude_degrees, latitude_degrees);
        self
    }

    #[must_use]
    pub const fn convention(mut self, convention: PixelConvention) -> Self {
        self.convention = convention;
        self
    }

    /// `VerticalCSTypeGeoKey` を書く。EPSG:3855 なら EGM2008。
    #[must_use]
    pub const fn vertical_cs_type(mut self, code: u16) -> Self {
        self.vertical_cs_type = Some(code);
        self
    }

    /// `GTModelTypeGeoKey` を差し替える。1 = 投影座標系、2 = 地理座標系。
    #[must_use]
    pub const fn model_type(mut self, model_type: u16) -> Self {
        self.model_type = model_type;
        self
    }

    #[must_use]
    pub const fn nodata(mut self, value: f32) -> Self {
        self.nodata = Some(value);
        self
    }

    /// 地理参照タグを一切書かない。ただの TIFF になる。
    #[must_use]
    pub const fn without_georeference(mut self) -> Self {
        self.georeferenced = false;
        self
    }

    /// TIFF バイト列を組み立てる。
    #[must_use]
    pub fn build(self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // --- ヘッダ（リトルエンディアン、クラシック TIFF） ---
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // IFD オフセットは後で埋める

        // --- 画素 ---
        let pixel_offset = bytes.len() as u32;
        for sample in &self.samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let pixel_byte_count = (self.samples.len() * 4) as u32;

        // --- IFD の外に置く値 ---
        pad_to(&mut bytes, 8);

        let mut georeference = None;
        if self.georeferenced {
            let scale_offset = bytes.len() as u32;
            for value in [self.pixel_size.0, self.pixel_size.1, 0.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }

            let tiepoint_offset = bytes.len() as u32;
            // [i, j, k, x, y, z]: ラスタ点 (0, 0) が地理座標 (origin) に対応する。
            for value in [0.0, 0.0, 0.0, self.origin.0, self.origin.1, 0.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }

            let raster_type = match self.convention {
                PixelConvention::Area => 1_u16,
                PixelConvention::Point => 2,
            };
            // ヘッダ 4 語 + キー。各キーは [KeyID, TIFFTagLocation, Count, Value]。
            //
            // **GeoKey は ID の昇順で並べる決まり。** 1024 → 1025 → 4096。
            let mut keys: Vec<u16> = vec![
                1,
                1,
                0,
                0, // キー数はあとで埋める
                1024,
                0,
                1,
                self.model_type,
                1025,
                0,
                1,
                raster_type,
            ];
            if let Some(code) = self.vertical_cs_type {
                keys.extend_from_slice(&[4096, 0, 1, code]);
            }
            #[allow(clippy::cast_possible_truncation, reason = "キーは数個。u16 に収まる")]
            {
                keys[3] = ((keys.len() - 4) / 4) as u16;
            }
            let keys_offset = bytes.len() as u32;
            for key in keys.iter().copied() {
                bytes.extend_from_slice(&key.to_le_bytes());
            }

            georeference = Some((
                scale_offset,
                tiepoint_offset,
                keys_offset,
                keys.len() as u32,
            ));
        }

        let nodata = self.nodata.map(|value| {
            pad_to(&mut bytes, 2);
            // GDAL は NUL 終端の ASCII で書く。カウントは NUL を含む。
            let text = format!("{value}\0");
            let offset = bytes.len() as u32;
            bytes.extend_from_slice(text.as_bytes());
            (offset, text.len() as u32)
        });

        // --- IFD ---
        pad_to(&mut bytes, 2);
        let ifd_offset = bytes.len() as u32;

        // (tag, field type, count, value/offset)。タグ昇順でなければならない。
        let mut entries: Vec<(u16, u16, u32, u32)> = vec![
            (256, LONG, 1, self.width),
            (257, LONG, 1, self.height),
            (258, SHORT, 1, 32), // BitsPerSample
            (259, SHORT, 1, 1),  // Compression: none
            (262, SHORT, 1, 1),  // PhotometricInterpretation: BlackIsZero
            (273, LONG, 1, pixel_offset),
            (277, SHORT, 1, 1),          // SamplesPerPixel
            (278, LONG, 1, self.height), // RowsPerStrip: 全体で 1 ストリップ
            (279, LONG, 1, pixel_byte_count),
            (284, SHORT, 1, 1), // PlanarConfiguration: chunky
            (339, SHORT, 1, 3), // SampleFormat: IEEE 浮動小数点
        ];

        if let Some((scale_offset, tiepoint_offset, keys_offset, key_count)) = georeference {
            entries.push((33_550, DOUBLE, 3, scale_offset));
            entries.push((33_922, DOUBLE, 6, tiepoint_offset));
            entries.push((34_735, SHORT, key_count, keys_offset));
        }
        if let Some((offset, length)) = nodata {
            entries.push((42_113, ASCII, length, offset));
        }
        entries.sort_by_key(|entry| entry.0);

        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, field_type, count, value) in entries {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&field_type.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            // 4 バイトに収まる値はこの場に置く。SHORT はリトルエンディアンの
            // 下位 2 バイトに入るので、u32 として書けばよい。
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // 次の IFD は無い

        bytes[4..8].copy_from_slice(&ifd_offset.to_le_bytes());
        bytes
    }
}

const SHORT: u16 = 3;
const LONG: u16 = 4;
const ASCII: u16 = 2;
const DOUBLE: u16 = 12;

fn pad_to(bytes: &mut Vec<u8>, alignment: usize) {
    // `usize::is_multiple_of` は 1.87 以降。ワークスペースの MSRV は 1.85。
    while bytes.len() % alignment != 0 {
        bytes.push(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builder_produces_a_little_endian_classic_tiff() {
        let bytes = GeoTiffBuilder::new(2, 2, vec![1.0, 2.0, 3.0, 4.0]).build();

        assert_eq!(&bytes[0..2], b"II");
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 42);

        let ifd_offset = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        assert!(ifd_offset > 0 && ifd_offset < bytes.len());
    }

    #[test]
    fn ifd_entries_are_sorted_by_tag() {
        // TIFF 仕様の要求。順序が崩れているとデコーダによっては読めない。
        let bytes = GeoTiffBuilder::new(2, 2, vec![0.0; 4]).nodata(-1.0).build();
        let ifd_offset = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let count = u16::from_le_bytes([bytes[ifd_offset], bytes[ifd_offset + 1]]) as usize;

        let mut previous = 0_u16;
        for index in 0..count {
            let base = ifd_offset + 2 + index * 12;
            let tag = u16::from_le_bytes([bytes[base], bytes[base + 1]]);
            assert!(tag > previous, "tag {tag} came after {previous}");
            previous = tag;
        }
    }

    #[test]
    fn omitting_the_georeference_drops_the_geo_tags() {
        let with = GeoTiffBuilder::new(2, 2, vec![0.0; 4]).build();
        let without = GeoTiffBuilder::new(2, 2, vec![0.0; 4])
            .without_georeference()
            .build();
        assert!(without.len() < with.len());
    }
}
