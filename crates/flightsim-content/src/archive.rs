use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

use rawzip::{CompressionMethod, ZipArchive, ZipArchiveEntryWayfinder};
use sha2::{Digest, Sha256};

use crate::install::{check_directory_path, validate_payload};
use crate::{
    Error, ImportPhase, ImportProgress, InstalledPackage, MANIFEST_NAME, MAX_ARCHIVE_BYTES,
    MAX_ARCHIVE_ENTRIES, MAX_COMPRESSION_RATIO, MAX_FILES, MAX_TOTAL_BYTES, Result, StagedPackage,
    manifest::{PortablePaths, file_byte_limit},
    notify, parse_manifest,
};

#[derive(Debug)]
struct Entry {
    path: String,
    size: u64,
    method: CompressionMethod,
    wayfinder: ZipArchiveEntryWayfinder,
}

/// Stage and validate a local ZIP. Returning false from the callback cancels and
/// removes only this operation's temporary directory. No installed version changes.
/// The caller owns a trusted local store root; hostile concurrent filesystem writers
/// are outside this API's threat model. Archive entries are always untrusted.
pub fn stage_zip_with_progress(
    zip_path: &Path,
    store_root: &Path,
    mut callback: impl FnMut(ImportProgress) -> bool,
) -> Result<StagedPackage> {
    let mut progress = ImportProgress {
        phase: ImportPhase::Inspecting,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
    };
    notify(&mut callback, progress)?;
    let metadata = std::fs::symlink_metadata(zip_path)?;
    if !metadata.is_file() || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(Error::Limit("archive file bytes / regular file required"));
    }
    let input = File::open(zip_path)?;
    let input_size = input.metadata()?.len();
    if input_size > MAX_ARCHIVE_BYTES {
        return Err(Error::Limit("archive file bytes"));
    }
    check_directory_path(store_root)?;
    std::fs::create_dir_all(store_root)?;
    check_directory_path(store_root)?;
    // Snapshot the untrusted input into private app-owned storage before parsing.
    // Preflight and decoder must see identical bytes even if the original ZIP changes.
    let mut snapshot = tempfile::Builder::new()
        .prefix(".archive-")
        .tempfile_in(store_root)?;
    copy_limited(input, snapshot.as_file_mut(), input_size, &mut |_| {
        notify(&mut callback, progress)
    })?;
    let mut file = snapshot.reopen()?;
    preflight_zip32(&mut file, &mut callback, progress)?;
    let mut buffer = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
    let mut local_buffer = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
    let archive = ZipArchive::from_file(file, &mut buffer)?;
    if archive.entries_hint() > MAX_ARCHIVE_ENTRIES as u64 {
        return Err(Error::Limit("archive entry count"));
    }
    let mut paths = PortablePaths::default();
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut ranges = Vec::new();
    let mut count = 0u64;
    let mut entries = archive.entries(&mut buffer);
    while let Some(entry) = entries.next_entry()? {
        count += 1;
        if count > MAX_ARCHIVE_ENTRIES as u64 {
            return Err(Error::Limit("archive entry count"));
        }
        let raw_path = entry.file_path();
        let raw_path = std::str::from_utf8(raw_path.as_bytes())
            .map_err(|_| Error::Invalid("non-UTF8 ZIP path".into()))?;
        let directory = entry.is_dir();
        let path = if directory {
            raw_path.strip_suffix('/').unwrap_or(raw_path)
        } else {
            raw_path
        };
        paths.add(path, directory)?;
        let mode = entry.mode().value() & 0o170000;
        if mode != 0 && mode != if directory { 0o040000 } else { 0o100000 } {
            return Err(Error::Invalid(
                "ZIP links and special files are forbidden".into(),
            ));
        }
        if entry.flags().bits() & !0x080e != 0 {
            return Err(Error::Invalid("encrypted or unsupported ZIP flags".into()));
        }
        let method = entry.compression_method();
        if method != CompressionMethod::STORE && method != CompressionMethod::DEFLATE {
            return Err(Error::Invalid(
                "only Stored and Deflate ZIP entries are supported".into(),
            ));
        }
        let size = entry.uncompressed_size_hint();
        let compressed = entry.compressed_size_hint();
        if directory {
            if size != 0 || compressed != 0 {
                return Err(Error::Invalid("nonempty directory entry".into()));
            }
            directories.push(path.to_owned());
        } else {
            if size > file_byte_limit(path)? {
                return Err(Error::Limit("entry bytes"));
            }
            if size > compressed.saturating_mul(MAX_COMPRESSION_RATIO) {
                return Err(Error::Limit("compression ratio"));
            }
            progress.bytes_total = progress
                .bytes_total
                .checked_add(size)
                .ok_or(Error::Limit("total bytes"))?;
            if progress.bytes_total > MAX_TOTAL_BYTES || files.len() > MAX_FILES {
                return Err(Error::Limit("total bytes / file count"));
            }
        }
        // Check local names/flags as well as the central directory. Never normalize away disagreement.
        let local = archive.get_entry(entry.wayfinder())?;
        let header = local.local_header(&mut local_buffer)?;
        if header.file_path().as_bytes() != raw_path.as_bytes()
            || header.flags() != entry.flags()
            || header.compression_method() != method
        {
            return Err(Error::Invalid("local/central ZIP header mismatch".into()));
        }
        let (data_start, data_end) = local.compressed_data_range();
        if entry.local_header_offset() >= data_start || data_end > archive.directory_offset() {
            return Err(Error::Invalid(
                "ZIP entry overlaps central directory".into(),
            ));
        }
        ranges.push((entry.local_header_offset(), data_end));
        if !directory {
            files.push(Entry {
                path: path.to_owned(),
                size,
                method,
                wayfinder: entry.wayfinder(),
            });
        }
        notify(&mut callback, progress)?;
    }
    if count != archive.entries_hint() {
        return Err(Error::Invalid("ZIP entry count mismatch".into()));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|w| w[0].1 > w[1].0) {
        return Err(Error::Invalid("overlapping ZIP entries".into()));
    }
    let manifest_entry = files
        .iter()
        .find(|f| f.path == MANIFEST_NAME)
        .ok_or_else(|| {
            Error::Invalid(
                "root manifest.json missing; raw terrain/repository ZIPs require packaging first"
                    .into(),
            )
        })?;
    let mut manifest_bytes = Vec::new();
    read_entry(&archive, manifest_entry, &mut manifest_bytes, &mut |_| {
        notify(&mut callback, progress)
    })?;
    let manifest = parse_manifest(&manifest_bytes)?;
    let expected: BTreeMap<_, _> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    if files.len() != expected.len() + 1 {
        return Err(Error::Invalid("archive/manifest file list mismatch".into()));
    }
    let allowed_dirs: BTreeSet<String> = files
        .iter()
        .flat_map(|f| {
            let parts: Vec<_> = f.path.split('/').collect();
            (1..parts.len())
                .map(|i| parts[..i].join("/"))
                .collect::<Vec<_>>()
        })
        .collect();
    if directories.iter().any(|d| !allowed_dirs.contains(d)) {
        return Err(Error::Invalid("undeclared ZIP directory".into()));
    }
    for entry in &files {
        if entry.path == MANIFEST_NAME {
            continue;
        }
        let record = expected
            .get(entry.path.as_str())
            .ok_or_else(|| Error::Invalid("undeclared ZIP file".into()))?;
        if entry.size != record.size_bytes {
            return Err(Error::Invalid(format!("size mismatch: {}", entry.path)));
        }
    }
    check_directory_path(store_root)?;
    std::fs::create_dir_all(store_root)?;
    check_directory_path(store_root)?;
    let temp = tempfile::Builder::new()
        .prefix(".import-")
        .tempdir_in(store_root)?;
    progress.phase = ImportPhase::Extracting;
    progress.files_total = files.len();
    for entry in &files {
        let target = temp.path().join(&entry.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = File::options().write(true).create_new(true).open(target)?;
        let before = progress.bytes_done;
        let mut hashed = HashingWriter {
            inner: &mut output,
            hash: Sha256::new(),
        };
        read_entry(&archive, entry, &mut hashed, &mut |done| {
            progress.bytes_done = before + done;
            notify(&mut callback, progress)
        })?;
        if entry.path != MANIFEST_NAME {
            let found = format!("{:x}", hashed.hash.finalize());
            if found != expected[entry.path.as_str()].sha256 {
                return Err(Error::Invalid(format!("SHA-256 mismatch: {}", entry.path)));
            }
        } else if format!("{:x}", hashed.hash.finalize()) != crate::sha256(&manifest_bytes) {
            return Err(Error::Invalid("manifest changed while importing".into()));
        }
        output.sync_all()?;
        progress.bytes_done = before + entry.size;
        progress.files_done += 1;
        notify(&mut callback, progress)?;
    }
    progress.phase = ImportPhase::Validating;
    progress.files_done = 0;
    for file in &manifest.files {
        validate_payload(temp.path(), file, &manifest)?;
        progress.files_done += 1;
        notify(&mut callback, progress)?;
    }
    progress.phase = ImportPhase::Ready;
    progress.files_done = progress.files_total;
    notify(&mut callback, progress)?;
    Ok(StagedPackage::new(
        temp,
        store_root.to_owned(),
        manifest,
        &manifest_bytes,
    ))
}

/// Convenience import with no progress callback. It does not activate a region.
pub fn install_zip(zip_path: &Path, store_root: &Path) -> Result<InstalledPackage> {
    stage_zip_with_progress(zip_path, store_root, |_| true)?.commit()
}

fn read_entry<W: Write>(
    archive: &ZipArchive<rawzip::FileReader>,
    entry: &Entry,
    output: &mut W,
    on_chunk: &mut impl FnMut(u64) -> Result<()>,
) -> Result<()> {
    let local = archive.get_entry(entry.wayfinder)?;
    let reader = local.reader();
    match entry.method {
        CompressionMethod::STORE => {
            copy_limited(local.verifying_reader(reader), output, entry.size, on_chunk)
        }
        CompressionMethod::DEFLATE => {
            // A decoder read may consume arbitrarily many empty Deflate blocks
            // before producing a byte. Check cancellation both outside the
            // decoder (output chunks) and on bounded compressed input reads.
            let callback = RefCell::new(on_chunk);
            let produced = Cell::new(0u64);
            let cancelled = Cell::new(false);
            let input = CallbackReader {
                inner: reader,
                before_read: || {
                    callback.borrow_mut()(produced.get()).map_err(|error| {
                        if matches!(error, Error::Cancelled) {
                            cancelled.set(true);
                        }
                        std::io::Error::other(error)
                    })
                },
            };
            let result = copy_limited(
                local.verifying_reader(flate2::read::DeflateDecoder::new(input)),
                output,
                entry.size,
                &mut |count| {
                    produced.set(count);
                    callback.borrow_mut()(count)
                },
            );
            if cancelled.get() {
                Err(Error::Cancelled)
            } else {
                result
            }
        }
        _ => Err(Error::Invalid("unsupported compression".into())),
    }
}

struct CallbackReader<R, F> {
    inner: R,
    before_read: F,
}
impl<R: Read, F: FnMut() -> std::io::Result<()>> Read for CallbackReader<R, F> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        (self.before_read)()?;
        let limit = output.len().min(32 * 1024);
        self.inner.read(&mut output[..limit])
    }
}

fn copy_limited(
    mut reader: impl Read,
    writer: &mut impl Write,
    expected: u64,
    on_chunk: &mut impl FnMut(u64) -> Result<()>,
) -> Result<()> {
    let mut buffer = [0u8; 32 * 1024];
    let mut count = 0u64;
    loop {
        on_chunk(count)?;
        // Read at most the remaining declared bytes plus one before rejecting a lying header.
        let allowed = usize::try_from((expected - count + 1).min(buffer.len() as u64))
            .unwrap_or(buffer.len());
        let n = reader.read(&mut buffer[..allowed])?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > expected {
            return Err(Error::Invalid("uncompressed ZIP size mismatch".into()));
        }
        writer.write_all(&buffer[..n])?;
    }
    if count != expected {
        return Err(Error::Invalid("truncated ZIP entry".into()));
    }
    Ok(())
}

struct HashingWriter<'a, W> {
    inner: &'a mut W,
    hash: Sha256,
}
impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

// v1's byte/count budgets fit ZIP32. Validate that strict envelope before asking
// rawzip to interpret offsets: no prepended/self-extracting data, ZIP64 offset
// arithmetic or offset-repair path is needed or permitted for this package format.
fn preflight_zip32(
    file: &mut File,
    callback: &mut impl FnMut(ImportProgress) -> bool,
    progress: ImportProgress,
) -> Result<()> {
    let len = file.metadata()?.len();
    if len < 22 {
        return Err(Error::Invalid("truncated ZIP32 envelope".into()));
    }
    file.seek(SeekFrom::End(-22))?;
    let mut end = [0u8; 22];
    file.read_exact(&mut end)?;
    let invalid = || {
        Error::Invalid("v1 requires single-disk ZIP32 without prefixes or archive comments".into())
    };
    if end[..4] != [0x50, 0x4b, 0x05, 0x06]
        || end[4..8] != [0; 4]
        || word(&end, 20) != 0
        || word(&end, 8) != word(&end, 10)
    {
        return Err(invalid());
    }
    let count = usize::from(word(&end, 10));
    if count == 0 || count > MAX_ARCHIVE_ENTRIES {
        return Err(Error::Limit("archive entry count"));
    }
    let directory = u64::from(dword(&end, 16));
    if directory + u64::from(dword(&end, 12)) != len - 22 {
        return Err(invalid());
    }
    if len >= 42 {
        file.seek(SeekFrom::End(-42))?;
        let mut signature = [0u8; 4];
        file.read_exact(&mut signature)?;
        if signature == [0x50, 0x4b, 0x06, 0x07] {
            return Err(invalid());
        }
    }
    file.seek(SeekFrom::Start(0))?;
    let mut signature = [0u8; 4];
    file.read_exact(&mut signature)?;
    if signature != [0x50, 0x4b, 0x03, 0x04] {
        return Err(invalid());
    }
    let mut position = directory;
    for _ in 0..count {
        notify(callback, progress)?;
        if position + 46 > len - 22 {
            return Err(invalid());
        }
        file.seek(SeekFrom::Start(position))?;
        let mut header = [0u8; 46];
        file.read_exact(&mut header)?;
        if header[..4] != [0x50, 0x4b, 0x01, 0x02]
            || word(&header, 34) != 0
            || dword(&header, 20) == u32::MAX
            || dword(&header, 24) == u32::MAX
            || u64::from(dword(&header, 42)) >= directory
        {
            return Err(invalid());
        }
        let name = usize::from(word(&header, 28));
        let extra = usize::from(word(&header, 30));
        let comment = usize::from(word(&header, 32));
        if name == 0 || name > 181 || extra > 4096 || comment > 4096 {
            return Err(Error::Limit("ZIP metadata bytes"));
        }
        position += 46 + (name + extra + comment) as u64;
        if position > len - 22 {
            return Err(invalid());
        }
        file.seek(SeekFrom::Current(name as i64))?;
        let mut extras = [0u8; 4096];
        file.read_exact(&mut extras[..extra])?;
        let mut cursor = 0;
        while cursor < extra {
            if cursor + 4 > extra {
                return Err(invalid());
            }
            let size = usize::from(word(&extras, cursor + 2));
            if word(&extras, cursor) == 1 || cursor + 4 + size > extra {
                return Err(invalid());
            }
            cursor += 4 + size;
        }
    }
    if position != len - 22 {
        return Err(invalid());
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}
fn word(bytes: &[u8], index: usize) -> u16 {
    u16::from_le_bytes([bytes[index], bytes[index + 1]])
}
fn dword(bytes: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(bytes[index..index + 4].try_into().unwrap())
}
