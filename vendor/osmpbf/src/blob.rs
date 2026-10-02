//! Read and decode blobs

use crate::block::{HeaderBlock, PrimitiveBlock};
use crate::error::{new_blob_error, new_error, new_protobuf_error, BlobError, ErrorKind, Result};
use crate::proto::fileformat;
use protobuf::Message;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use flate2::{Decompress, FlushDecompress, Status};

/// Maximum allowed [`BlobHeader`] size in bytes.
pub static MAX_BLOB_HEADER_SIZE: u64 = 64 * 1024;

/// Maximum allowed uncompressed [`Blob`] content size in bytes.
pub static MAX_BLOB_MESSAGE_SIZE: u64 = 32 * 1024 * 1024;

/// The content type of a blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlobType<'a> {
    /// Blob contains a [`HeaderBlock`].
    OsmHeader,
    /// Blob contains a [`PrimitiveBlock`].
    OsmData,
    /// An unknown blob type with the given string identifier.
    /// Parsers should ignore unknown blobs they do not expect.
    Unknown(&'a str),
}

impl<'a> BlobType<'a> {
    pub const fn as_str(&self) -> &'a str {
        match self {
            Self::OsmHeader => "OSMHeader",
            Self::OsmData => "OSMData",
            Self::Unknown(x) => x,
        }
    }
}

//TODO rename variants to fit proto files
/// The decoded content of a blob (analogous to [`BlobType`]).
#[derive(Clone, Debug)]
pub enum BlobDecode<'a> {
    /// Blob contains a [`HeaderBlock`].
    OsmHeader(Box<HeaderBlock>),
    /// Blob contains a [`PrimitiveBlock`].
    OsmData(PrimitiveBlock),
    /// An unknown blob type with the given string identifier.
    /// Parsers should ignore unknown blobs they do not expect.
    Unknown(&'a str),
}

/// The offset of a blob in bytes from stream start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteOffset(pub u64);

/// A blob.
///
/// A PBF file consists of a sequence of blobs. This type supports decoding the content of a blob
/// to different types of blocks that are usually more interesting to the user.
#[derive(Clone, Debug)]
pub struct Blob {
    header: fileformat::BlobHeader,
    blob: fileformat::Blob,
    offset: Option<ByteOffset>,
}

impl Blob {
    fn new(
        header: fileformat::BlobHeader,
        blob: fileformat::Blob,
        offset: Option<ByteOffset>,
    ) -> Blob {
        Blob {
            header,
            blob,
            offset,
        }
    }

    /// Decodes the Blob and tries to obtain the inner content (usually a [`HeaderBlock`] or a
    /// [`PrimitiveBlock`]). This operation might involve an expensive decompression step.
    pub fn decode(&self) -> Result<BlobDecode<'_>> {
        match self.get_type() {
            BlobType::OsmHeader => {
                let block = Box::new(self.to_headerblock()?);
                Ok(BlobDecode::OsmHeader(block))
            }
            BlobType::OsmData => {
                let block = self.to_primitiveblock()?;
                Ok(BlobDecode::OsmData(block))
            }
            BlobType::Unknown(x) => Ok(BlobDecode::Unknown(x)),
        }
    }

    /// Validated uncompressed byte count, without decompressing the blob.
    ///
    /// Consumers can charge a cumulative work budget before `decode()`. Raw data
    /// uses its actual length; zlib uses the required bounded `raw_size`. A
    /// successful zlib decode still verifies its exact output/input lengths and
    /// stream end, so this metadata cannot authorize an oversized expansion.
    /// Unknown blob types can be skipped using `get_type()` before this call.
    pub fn uncompressed_size(&self) -> Result<usize> {
        uncompressed_blob_size(&self.blob)
    }

    /// Returns the type of a blob without decoding its content.
    pub fn get_type(&self) -> BlobType<'_> {
        match self.header.type_() {
            x if x == BlobType::OsmHeader.as_str() => BlobType::OsmHeader,
            x if x == BlobType::OsmData.as_str() => BlobType::OsmData,
            x => BlobType::Unknown(x),
        }
    }

    /// Returns the byte offset of the blob from the start of its source stream.
    /// This might be [`None`] if the source stream does not implement [`Seek`].
    pub fn offset(&self) -> Option<ByteOffset> {
        self.offset
    }

    /// Tries to decode the blob to a [`HeaderBlock`]. This operation might involve an expensive
    /// decompression step.
    pub fn to_headerblock(&self) -> Result<HeaderBlock> {
        decode_blob(&self.blob).map(HeaderBlock::new)
    }

    /// Tries to decode the blob to a [`PrimitiveBlock`]. This operation might involve an expensive
    /// decompression step.
    pub fn to_primitiveblock(&self) -> Result<PrimitiveBlock> {
        decode_blob(&self.blob).and_then(PrimitiveBlock::new)
    }
}

/// A blob header.
///
/// Just contains information about the size and type of the following [`Blob`].
#[derive(Clone, Debug)]
pub struct BlobHeader {
    header: fileformat::BlobHeader,
}

impl BlobHeader {
    fn new(header: fileformat::BlobHeader) -> Self {
        BlobHeader { header }
    }

    /// Returns the type of the following blob.
    pub fn blob_type(&self) -> BlobType<'_> {
        match self.header.type_() {
            "OSMHeader" => BlobType::OsmHeader,
            "OSMData" => BlobType::OsmData,
            x => BlobType::Unknown(x),
        }
    }

    /// Returns the size of the following blob in bytes.
    pub fn get_blob_size(&self) -> i32 {
        self.header.datasize()
    }
}

/// A reader for PBF files that allows iterating over [`Blob`]s.
#[derive(Clone, Debug)]
pub struct BlobReader<R: Read + Send> {
    reader: R,
    /// Current reader offset in bytes from the start of the stream.
    offset: Option<ByteOffset>,
    last_blob_ok: bool,
}

impl<R: Read + Send> BlobReader<R> {
    /// Creates a new `BlobReader`.
    ///
    /// # Example
    /// ```
    /// use osmpbf::*;
    ///
    /// # fn foo() -> Result<()> {
    /// let f = std::fs::File::open("tests/test.osm.pbf")?;
    /// let buf_reader = std::io::BufReader::new(f);
    ///
    /// let reader = BlobReader::new(buf_reader);
    ///
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    pub fn new(reader: R) -> BlobReader<R> {
        BlobReader {
            reader,
            offset: None,
            last_blob_ok: true,
        }
    }

    fn read_blob_header(&mut self) -> Option<Result<fileformat::BlobHeader>> {
        let mut prefix = [0_u8; 4];
        // EOF is legal only before the first byte, not part-way through a prefix.
        loop {
            match self.reader.read(&mut prefix[..1]) {
                Ok(0) => return None,
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.last_blob_ok = false;
                    return Some(Err(error.into()));
                }
            }
        }
        if let Err(error) = self.reader.read_exact(&mut prefix[1..]) {
            self.last_blob_ok = false;
            return Some(Err(error.into()));
        }
        let header_size = u64::from(u32::from_be_bytes(prefix));
        self.offset = self.offset.and_then(|x| x.0.checked_add(4).map(ByteOffset));

        if header_size >= MAX_BLOB_HEADER_SIZE {
            self.last_blob_ok = false;
            return Some(Err(new_blob_error(BlobError::HeaderTooBig {
                size: header_size,
            })));
        }

        let bytes = match read_exact_bytes(&mut self.reader, header_size as usize) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.last_blob_ok = false;
                return Some(Err(error));
            }
        };
        let header = match fileformat::BlobHeader::parse_from_bytes(&bytes) {
            Ok(header) => header,
            Err(e) => {
                self.offset = None;
                self.last_blob_ok = false;
                return Some(Err(new_protobuf_error(e, "blob header")));
            }
        };

        if let Err(error) = validate_blob_size(header.datasize()) {
            self.last_blob_ok = false;
            return Some(Err(error));
        }
        self.offset = self
            .offset
            .and_then(|x| x.0.checked_add(header_size).map(ByteOffset));

        Some(Ok(header))
    }
}

impl BlobReader<BufReader<File>> {
    /// Tries to open the file at the given path and constructs a `BlobReader` from this.
    /// If there are no errors, each blob will have a valid ([`Some`]) offset.
    ///
    /// # Errors
    /// Returns the same errors that `std::fs::File::open` returns.
    ///
    /// # Example
    /// ```
    /// use osmpbf::*;
    ///
    /// # fn foo() -> Result<()> {
    /// let reader = BlobReader::from_path("tests/test.osm.pbf")?;
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let f = File::open(path)?;
        let reader = BufReader::new(f);

        Ok(BlobReader {
            reader,
            offset: Some(ByteOffset(0)),
            last_blob_ok: true,
        })
    }
}

impl<R: Read + Send> Iterator for BlobReader<R> {
    type Item = Result<Blob>;

    fn next(&mut self) -> Option<Self::Item> {
        // Stop iteration if there was an error.
        if !self.last_blob_ok {
            return None;
        }

        let prev_offset = self.offset;

        let header = match self.read_blob_header() {
            Some(Ok(header)) => header,
            Some(Err(err)) => return Some(Err(err)),
            None => return None,
        };

        let bytes = match read_exact_bytes(&mut self.reader, header.datasize() as usize) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.last_blob_ok = false;
                return Some(Err(error));
            }
        };
        let blob = match fileformat::Blob::parse_from_bytes(&bytes) {
            Ok(blob) => blob,
            Err(e) => {
                self.offset = None;
                self.last_blob_ok = false;
                return Some(Err(new_protobuf_error(e, "blob content")));
            }
        };

        self.offset = self
            .offset
            .and_then(|x| x.0.checked_add(header.datasize() as u64).map(ByteOffset));

        Some(Ok(Blob::new(header, blob, prev_offset)))
    }
}

impl<R: Read + Seek + Send> BlobReader<R> {
    /// Creates a new `BlobReader` from the given reader that is seekable and will be initialized
    /// with a valid offset.
    ///
    /// # Example
    /// ```
    /// use osmpbf::*;
    ///
    /// # fn foo() -> Result<()> {
    /// let f = std::fs::File::open("tests/test.osm.pbf")?;
    /// let buf_reader = std::io::BufReader::new(f);
    ///
    /// let mut reader = BlobReader::new_seekable(buf_reader)?;
    /// let first_blob = reader.next().unwrap()?;
    ///
    /// assert_eq!(first_blob.offset(), Some(ByteOffset(0)));
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    pub fn new_seekable(mut reader: R) -> Result<BlobReader<R>> {
        let pos = reader.stream_position()?;

        Ok(BlobReader {
            reader,
            offset: Some(ByteOffset(pos)),
            last_blob_ok: true,
        })
    }

    /// Read and return the [`Blob`] at the given offset. If successful, the cursor of the stream is
    /// positioned at the start of the next [`Blob`].
    ///
    /// # Example
    /// ```
    /// use osmpbf::*;
    ///
    /// # fn foo() -> Result<()> {
    /// let mut reader = BlobReader::from_path("tests/test.osm.pbf")?;
    /// let first_blob = reader.next().unwrap()?;
    /// let second_blob = reader.next().unwrap()?;
    ///
    /// let offset = first_blob.offset().unwrap();
    /// let first_blob_again = reader.blob_from_offset(offset)?;
    /// assert_eq!(first_blob.offset(), first_blob_again.offset());
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    pub fn blob_from_offset(&mut self, pos: ByteOffset) -> Result<Blob> {
        self.seek(pos)?;
        self.next().unwrap_or_else(|| {
            Err(new_error(ErrorKind::Io(::std::io::Error::new(
                ::std::io::ErrorKind::UnexpectedEof,
                "no blob at this stream position",
            ))))
        })
    }

    /// Seek to an offset in bytes from the start of the stream.
    ///
    /// # Example
    /// ```
    /// use osmpbf::*;
    ///
    /// # fn foo() -> Result<()> {
    /// let mut reader = BlobReader::from_path("tests/test.osm.pbf")?;
    /// let first_blob = reader.next().unwrap()?;
    /// let second_blob = reader.next().unwrap()?;
    ///
    /// reader.seek(first_blob.offset().unwrap())?;
    ///
    /// let first_blob_again = reader.next().unwrap()?;
    /// assert_eq!(first_blob.offset(), first_blob_again.offset());
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    pub fn seek(&mut self, pos: ByteOffset) -> Result<()> {
        match self.reader.seek(SeekFrom::Start(pos.0)) {
            Ok(offset) => {
                self.offset = Some(ByteOffset(offset));
                Ok(())
            }
            Err(e) => {
                self.offset = None;
                Err(e.into())
            }
        }
    }

    /// Seek to an offset in bytes. (See `std::io::Seek`)
    pub fn seek_raw(&mut self, pos: SeekFrom) -> Result<u64> {
        match self.reader.seek(pos) {
            Ok(offset) => {
                self.offset = Some(ByteOffset(offset));
                Ok(offset)
            }
            Err(e) => {
                self.offset = None;
                Err(e.into())
            }
        }
    }

    /// Read and return next [`BlobHeader`] but skip the following [`Blob`]. This allows really fast
    /// iteration of the PBF structure if only the byte offset and [`BlobType`] are important.
    /// On success, returns the [`BlobHeader`] and the byte offset of the header which can also be
    /// used as an offset for reading the entire [`Blob`] (including header).
    pub fn next_header_skip_blob(&mut self) -> Option<Result<(BlobHeader, Option<ByteOffset>)>> {
        // Stop iteration if there was an error.
        if !self.last_blob_ok {
            return None;
        }

        let prev_offset = self.offset;

        // read header
        let header = match self.read_blob_header() {
            Some(Ok(header)) => header,
            Some(Err(err)) => return Some(Err(err)),
            None => return None,
        };

        // skip blob (which also adjusts self.offset)
        if let Err(err) = self.seek_raw(SeekFrom::Current(header.datasize() as i64)) {
            self.last_blob_ok = false;
            return Some(Err(err));
        }

        Some(Ok((BlobHeader::new(header), prev_offset)))
    }
}

impl BlobReader<BufReader<File>> {
    /// Creates a new `BlobReader` from the given path that is seekable and will be initialized
    /// with a valid offset.
    ///
    /// # Example
    /// ```
    /// use osmpbf::*;
    ///
    /// # fn foo() -> Result<()> {
    /// let mut reader = BlobReader::seekable_from_path("tests/test.osm.pbf")?;
    /// let first_blob = reader.next().unwrap()?;
    ///
    /// assert_eq!(first_blob.offset(), Some(ByteOffset(0)));
    /// # Ok(())
    /// # }
    /// # foo().unwrap();
    /// ```
    pub fn seekable_from_path<P: AsRef<Path>>(path: P) -> Result<BlobReader<BufReader<File>>> {
        let f = File::open(path.as_ref())?;
        let buf_reader = BufReader::new(f);
        Self::new_seekable(buf_reader)
    }
}

fn uncompressed_blob_size(blob: &fileformat::Blob) -> Result<usize> {
    if blob.has_raw() {
        let size = blob.raw().len() as u64;
        if size >= MAX_BLOB_MESSAGE_SIZE {
            return Err(new_blob_error(BlobError::MessageTooBig { size }));
        }
        if blob.has_raw_size() && i64::from(blob.raw_size()) != size as i64 {
            return Err(crate::validate::invalid(
                "blob raw_size does not match raw data",
            ));
        }
        Ok(blob.raw().len())
    } else if blob.has_zlib_data() {
        if !blob.has_raw_size()
            || blob.raw_size() < 0
            || blob.raw_size() as u64 >= MAX_BLOB_MESSAGE_SIZE
        {
            return Err(crate::validate::invalid(
                "invalid or missing compressed blob raw_size",
            ));
        }
        Ok(blob.raw_size() as usize)
    } else {
        Err(new_blob_error(BlobError::Empty))
    }
}

pub(crate) fn decode_blob<T: Message>(blob: &fileformat::Blob) -> Result<T> {
    let length = uncompressed_blob_size(blob)?;
    if blob.has_raw() {
        T::parse_from_bytes(blob.raw()).map_err(|e| new_protobuf_error(e, "raw blob data"))
    } else if blob.has_zlib_data() {
        // A Read decoder can return EOF after producing every payload byte even
        // when the zlib checksum trailer is missing. Require the low-level
        // decoder's StreamEnd, exact output size, and complete input consumption.
        // One spare output byte distinguishes an oversized stream from a valid
        // stream whose decoded length exactly fills its advertised buffer.
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length + 1)
            .map_err(|_| crate::validate::invalid("could not allocate decompressed blob"))?;
        bytes.resize(length + 1, 0);
        let mut decoder = Decompress::new(true);
        let status = decoder
            .decompress(blob.zlib_data(), &mut bytes, FlushDecompress::Finish)
            .map_err(|error| crate::validate::invalid(format!("invalid zlib blob: {error}")))?;
        if status != Status::StreamEnd
            || decoder.total_out() != length as u64
            || decoder.total_in() != blob.zlib_data().len() as u64
        {
            return Err(crate::validate::invalid(
                "incomplete zlib stream, trailing compressed data, or raw_size mismatch",
            ));
        }
        bytes.truncate(length);
        T::parse_from_bytes(&bytes).map_err(|e| new_protobuf_error(e, "blob zlib data"))
    } else {
        Err(new_blob_error(BlobError::Empty))
    }
}

pub(crate) fn validate_blob_size(size: i32) -> Result<()> {
    if size <= 0 || size as u64 >= MAX_BLOB_MESSAGE_SIZE {
        return Err(crate::validate::invalid(
            "invalid or oversized serialized blob length",
        ));
    }
    Ok(())
}

fn read_exact_bytes(reader: &mut impl Read, length: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| crate::validate::invalid("could not allocate PBF frame"))?;
    bytes.resize(length, 0);
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncompressed_size_checks_raw_and_compressed_metadata() {
        let mut raw = fileformat::Blob::new();
        raw.set_raw(vec![1, 2, 3]);
        let make = |value| Blob::new(fileformat::BlobHeader::new(), value, None);
        assert_eq!(make(raw.clone()).uncompressed_size().unwrap(), 3);
        raw.set_raw_size(3);
        assert_eq!(make(raw.clone()).uncompressed_size().unwrap(), 3);
        raw.set_raw_size(4);
        assert!(make(raw).uncompressed_size().is_err());
        let mut compressed = fileformat::Blob::new();
        compressed.set_zlib_data(vec![0]);
        assert!(make(compressed.clone()).uncompressed_size().is_err());
        for invalid in [-1, 33_554_432, i32::MAX] {
            compressed.set_raw_size(invalid);
            assert!(make(compressed.clone()).uncompressed_size().is_err());
        }
        compressed.set_raw_size(123);
        assert_eq!(make(compressed.clone()).uncompressed_size().unwrap(), 123);
        assert!(decode_blob::<crate::proto::osmformat::PrimitiveBlock>(&compressed).is_err());
        assert!(make(fileformat::Blob::new()).uncompressed_size().is_err());
    }

    #[test]
    fn test_get_type() {
        let pairs = [
            ("", BlobType::Unknown("")),
            ("abc", BlobType::Unknown("abc")),
            ("OSMHeader", BlobType::OsmHeader),
            ("OSMData", BlobType::OsmData),
        ];

        for (string, blob_type) in &pairs {
            let mut ff_header = fileformat::BlobHeader::new();
            ff_header.set_type(string.to_string());
            let ff_blob = fileformat::Blob::new();

            let blob = Blob::new(ff_header, ff_blob, None);
            assert_eq!(blob.get_type(), *blob_type);
        }
    }
}
