// SPDX-License-Identifier: MIT OR Apache-2.0
//! Compare separately built scalar and x86-feature JPEG decoders byte-for-byte.
//! Inputs are explicit local fixtures; nothing is downloaded or published.
use std::{fs, io::Cursor, path::PathBuf};
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

fn main() {
    let mut args = std::env::args().skip(1);
    let operation = args.next().expect("write-reference or compare-reference");
    let directory = PathBuf::from(args.next().expect("private comparison directory"));
    assert!(matches!(
        operation.as_str(),
        "write-reference" | "compare-reference"
    ));
    assert_eq!(
        cfg!(feature = "accelerated"),
        operation == "compare-reference",
        "write baseline with --no-default-features, compare with defaults"
    );
    if operation == "write-reference" {
        fs::create_dir_all(&directory).expect("create reference directory");
    }
    let mut files = 0;
    let mut channels = 0;
    for name in args {
        let bytes = fs::read(&name).expect("read supplied JPEG fixture");
        let mut decoder =
            JpegDecoder::new_with_options(Cursor::new(&bytes), DecoderOptions::new_fast());
        let actual = decoder.decode().expect("JPEG decode");
        let reference = directory.join(format!("{files}.decoded"));
        if operation == "write-reference" {
            fs::write(reference, &actual).expect("write private decoded reference");
        } else {
            let expected = fs::read(reference).expect("read scalar decoded reference");
            assert_eq!(actual, expected, "decoded bytes differ: {name}");
        }
        channels += actual.len();
        files += 1;
    }
    assert!(files > 0, "supply at least one JPEG fixture");
    println!("PASS: {operation}, {files} JPEG fixtures, {channels} decoded channel bytes, x86_feature={}", cfg!(feature = "accelerated"));
}
