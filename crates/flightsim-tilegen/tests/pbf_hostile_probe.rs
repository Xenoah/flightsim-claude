//! 敵対的 PBF に対する `airportgen` の境界の検査。
//!
//! # 何を確かめるか
//!
//! **細工・破損した PBF で panic せず、通常のエラーとして報告すること。**
//!
//! Upstream `osmpbf 0.3.7` は malformed protobuf の極端な delta / offset を
//! unchecked な i64 算術で復号する箇所があり、debug で panic、
//! release で wrap しうる（Issue #23）。正規提供元の PBF では起きないが、
//! **利用者が拾ってきた PBF は untrusted 入力**である。
//!
//! The local safety fork must reject malformed inner fields with ordinary
//! errors in both debug and release. Independently encoded valid data must keep
//! the original output bytes and reason-specific skip reports.

use flightsim_tilegen::airport::generate_airport_database;

#[path = "support/pbf.rs"]
mod pbf;

/// 一時ディレクトリに壊れた PBF を書いて、読み込みを試す。
fn probe(name: &str, bytes: &[u8]) -> Result<(), String> {
    let directory = std::env::temp_dir().join(format!(
        "flightsim-pbf-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("temp dir");
    let path = directory.join(format!("{name}.osm.pbf"));
    std::fs::write(&path, bytes).expect("write the hostile fixture");

    let output = directory.join("out.fsap");
    let previous = b"existing airport database must survive failed conversion";
    std::fs::write(&output, previous).expect("existing output");
    let outcome = generate_airport_database(&path, &output)
        .map(|_| ())
        .map_err(|error| error.to_string());
    assert_eq!(std::fs::read(&path).expect("input survives"), bytes);
    if outcome.is_err() {
        assert_eq!(std::fs::read(&output).expect("output survives"), previous);
    }
    std::fs::remove_dir_all(&directory).ok();
    outcome
}

fn rejects_group(name: &str, group: &[u8], block_extra: &[u8], reason: &str) {
    let message = probe(name, &pbf::pbf(&pbf::block(group, block_extra)))
        .expect_err("malformed primitive blocks must return a normal error");
    assert!(
        message.contains(reason),
        "{name}: expected {reason:?}, got {message}"
    );
}

#[test]
fn overflowing_dense_deltas_are_rejected_before_iteration() {
    for (name, ids, lat, lon) in [
        ("id", [i64::MAX, 1], [0, 0], [0, 0]),
        ("latitude", [1, 1], [i64::MAX, 1], [0, 0]),
        ("longitude", [1, 1], [0, 0], [i64::MIN, -1]),
    ] {
        rejects_group(
            name,
            &pbf::bytes(2, &pbf::dense(&ids, &lat, &lon, &[])),
            &pbf::integer(17, 1),
            "overflow",
        );
    }
}

#[test]
fn overflowing_coordinate_scale_and_offset_are_rejected() {
    rejects_group(
        "node-scale",
        &pbf::bytes(1, &pbf::node(1, i64::MAX, 0, &[])),
        &[],
        "overflow",
    );
    rejects_group(
        "node-offset",
        &pbf::bytes(1, &pbf::node(1, 1, 0, &[])),
        &pbf::integer(19, i64::MAX as u64),
        "overflow",
    );
    rejects_group(
        "dense-scale",
        &pbf::bytes(2, &pbf::dense(&[1], &[i64::MAX], &[0], &[])),
        &[],
        "overflow",
    );
}

#[test]
fn overflowing_way_and_relation_deltas_are_rejected() {
    rejects_group(
        "way-delta",
        &pbf::bytes(3, &pbf::way(1, &[i64::MAX, 1], &[(1, 2)])),
        &[],
        "overflow",
    );
    rejects_group(
        "relation-delta",
        &pbf::bytes(4, &pbf::relation(&[i64::MIN, -1], &[0, 0], &[1, 1])),
        &[],
        "overflow",
    );
}

#[test]
fn overflowing_dense_metadata_is_rejected_even_when_airportgen_does_not_use_it() {
    for (name, field, values) in [
        ("timestamp", 2, [i64::MAX, 1]),
        ("changeset", 3, [i64::MAX, 1]),
        ("uid", 4, [i64::from(i32::MAX), 1]),
    ] {
        let info = [pbf::packed(1, &[1, 1]), pbf::deltas(field, &values)].concat();
        let dense = [
            pbf::dense(&[1, 1], &[0, 0], &[0, 0], &[]),
            pbf::bytes(5, &info),
        ]
        .concat();
        rejects_group(name, &pbf::bytes(2, &dense), &[], "overflow");
    }
}

#[test]
fn invalid_string_table_indices_are_errors_instead_of_silently_lost_tags() {
    rejects_group(
        "node-index",
        &pbf::bytes(1, &pbf::node(1, 0, 0, &[(99, 1)])),
        &[],
        "stringtable index",
    );
    rejects_group(
        "way-index",
        &pbf::bytes(3, &pbf::way(1, &[1, 1], &[(1, 99)])),
        &[],
        "stringtable index",
    );
    rejects_group(
        "dense-index",
        &pbf::bytes(2, &pbf::dense(&[1], &[0], &[0], &[u64::MAX, 1, 0])),
        &[],
        "stringtable index",
    );
    rejects_group(
        "relation-role",
        &pbf::bytes(4, &pbf::relation(&[1], &[99], &[1])),
        &[],
        "stringtable index",
    );
}

#[test]
fn invalid_utf8_and_unpaired_tag_arrays_are_errors() {
    let table = [pbf::bytes(1, b""), pbf::bytes(1, &[0xff])].concat();
    let group = pbf::bytes(1, &pbf::node(1, 0, 0, &[(1, 0)]));
    let block = [pbf::bytes(1, &table), pbf::bytes(2, &group)].concat();
    assert!(
        probe("invalid-utf8", &pbf::pbf(&block))
            .unwrap_err()
            .contains("UTF-8")
    );
    let node = [pbf::node(1, 0, 0, &[]), pbf::packed(2, &[1])].concat();
    rejects_group("unpaired-tags", &pbf::bytes(1, &node), &[], "unequal tag");
}

#[test]
fn dense_delimiters_and_parallel_arrays_are_strict() {
    rejects_group(
        "dense-length",
        &pbf::bytes(2, &pbf::dense(&[1, 1], &[0], &[0], &[])),
        &[],
        "unequal dense",
    );
    rejects_group(
        "dense-delimiter",
        &pbf::bytes(2, &pbf::dense(&[1], &[0], &[0], &[1, 2])),
        &[],
        "delimiter",
    );
    rejects_group(
        "dense-extra",
        &pbf::bytes(2, &pbf::dense(&[1], &[0], &[0], &[0, 0])),
        &[],
        "extra dense",
    );
    rejects_group(
        "relation-length",
        &pbf::bytes(4, &pbf::relation(&[1, 2], &[0], &[1])),
        &[],
        "unequal relation",
    );
    rejects_group(
        "relation-type",
        &pbf::bytes(4, &pbf::relation(&[1], &[0], &[99])),
        &[],
        "unknown relation",
    );
}

#[test]
fn a_valid_pbf_with_a_partial_trailing_prefix_is_rejected() {
    for length in 1..4 {
        let mut bytes = pbf::valid_airport();
        bytes.extend(std::iter::repeat_n(0, length));
        assert!(probe("trailing-prefix", &bytes).is_err());
    }
}

#[test]
fn serialized_blob_lengths_are_bounded_and_fully_present() {
    for size in [
        u64::MAX,
        0,
        32 * 1024 * 1024,
        u64::try_from(i32::MAX).unwrap(),
    ] {
        let header = [pbf::bytes(1, b"OSMData"), pbf::integer(3, size)].concat();
        let length = u32::try_from(header.len()).unwrap();
        let data = [length.to_be_bytes().to_vec(), header, vec![0; 10]].concat();
        assert!(probe("blob-length", &data).is_err());
    }
    let mut bytes = pbf::valid_airport();
    bytes.pop();
    assert!(probe("truncated-valid", &bytes).is_err());
}

#[test]
fn compressed_blobs_check_declared_size_and_zlib_integrity() {
    let block = pbf::block(&pbf::bytes(1, &pbf::node(1, 0, 0, &[])), &[]);
    let compressed = pbf::zlib_stored(&block);
    let size = block.len() as u64;
    let valid = pbf::frame(
        "OSMData",
        &[pbf::integer(2, size), pbf::bytes(3, &compressed)].concat(),
    );
    assert!(probe("valid-zlib", &valid).is_ok());
    // read::ZlibDecoder can produce all payload bytes without rejecting a
    // missing Adler32 trailer. Successful Read EOF is not proof of StreamEnd.
    for removed in 1..=4 {
        let truncated = &compressed[..compressed.len() - removed];
        let bytes = pbf::frame(
            "OSMData",
            &[pbf::integer(2, size), pbf::bytes(3, truncated)].concat(),
        );
        assert!(probe("zlib-truncated-trailer", &bytes).is_err());
    }
    for suffix in [vec![0], pbf::zlib_stored(&block)] {
        let trailing = [compressed.clone(), suffix].concat();
        let bytes = pbf::frame(
            "OSMData",
            &[pbf::integer(2, size), pbf::bytes(3, &trailing)].concat(),
        );
        assert!(probe("zlib-trailing-data", &bytes).is_err());
    }
    for wrong_size in [0, size - 1, size + 1, u64::MAX, 32 * 1024 * 1024] {
        let bytes = pbf::frame(
            "OSMData",
            &[pbf::integer(2, wrong_size), pbf::bytes(3, &compressed)].concat(),
        );
        assert!(probe("zlib-size", &bytes).is_err());
    }
    let mut corrupt = compressed;
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    assert!(
        probe(
            "zlib-checksum",
            &pbf::frame(
                "OSMData",
                &[pbf::integer(2, size), pbf::bytes(3, &corrupt)].concat()
            )
        )
        .is_err()
    );
}

#[test]
fn valid_airport_output_and_reason_specific_skips_remain_deterministic() {
    let directory = tempfile::tempdir().expect("temp directory");
    let input = directory.path().join("valid.osm.pbf");
    let output = directory.path().join("out.fsairports");
    std::fs::write(&input, pbf::valid_airport()).expect("fixture");
    let first = generate_airport_database(&input, &output).expect("valid conversion");
    let bytes = std::fs::read(&output).expect("first output");
    // Captured with unmodified main 2d2295b / osmpbf 0.3.7 from the same
    // independently encoded synthetic PBF, before applying the safety fork.
    assert_eq!(bytes, include_bytes!("fixtures/airport-valid.fsairports"));
    let second = generate_airport_database(&input, &output).expect("second conversion");
    assert_eq!(bytes, std::fs::read(&output).expect("second output"));
    assert_eq!(first, second);
    assert_eq!(first.runway_ways_seen, 5);
    assert_eq!(first.runways_written, 1);
    assert_eq!(first.skipped_closed, 1);
    assert_eq!(first.skipped_areas, 1);
    assert_eq!(first.skipped_missing_nodes, 1);
    assert_eq!(first.skipped_bad_coordinates, 1);
    assert_eq!(first.taxiways_written, 1);
    assert_eq!(first.taxiway_segments_written, 1);
}

#[test]
fn an_empty_file_is_an_error_not_a_panic() {
    assert!(probe("empty", &[]).is_err(), "an empty PBF was accepted");
}

#[test]
fn random_bytes_are_an_error_not_a_panic() {
    // 決定論的な擬似乱数。**乱数を引かない**（再現できないと追えない）。
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for length in [1_usize, 16, 64, 1024, 8192] {
        #[allow(clippy::cast_possible_truncation, reason = "下位 8 bit だけ使う")]
        let bytes: Vec<u8> = (0..length).map(|_| (next() & 0xFF) as u8).collect();
        // Ok でも Err でもよい。panic しないことだけを見る。
        let _ = probe("random", &bytes);
    }
}

#[test]
fn a_blob_header_claiming_an_absurd_size_is_an_error_not_a_panic() {
    // PBF は [4 バイト BE の header 長][BlobHeader][Blob] の繰り返し。
    // **header 長に巨大な値を宣言する。** これを信じて確保すると落ちる。
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    bytes.extend_from_slice(&[0x00; 64]);
    assert!(
        probe("absurd-header", &bytes).is_err(),
        "a blob header claiming u32::MAX bytes was accepted"
    );
}

#[test]
fn a_truncated_blob_is_an_error_not_a_panic() {
    // 長さは正当だが、その後ろが足りない。ダウンロード失敗で普通に起きる。
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&32_u32.to_be_bytes());
    bytes.extend_from_slice(&[0x0A, 0x04, b'O', b'S', b'M', b'H']);
    assert!(
        probe("truncated", &bytes).is_err(),
        "a truncated blob was accepted"
    );
}

#[test]
fn protobuf_varint_bombs_are_an_error_not_a_panic() {
    // 終端しない varint。**10 バイトを超える varint は不正**で、
    // 素朴な実装は無限に読み進める。
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&64_u32.to_be_bytes());
    // continuation bit を立て続ける。
    bytes.extend(std::iter::repeat_n(0xFF_u8, 64));
    assert!(
        probe("varint-bomb", &bytes).is_err(),
        "an unterminated varint was accepted"
    );
}

#[test]
fn a_header_shorter_than_the_length_prefix_is_an_error_not_a_panic() {
    // 4 バイトに満たないファイル。
    for length in 0..4_usize {
        let bytes = vec![0xFF_u8; length];
        assert!(
            probe("short", &bytes).is_err(),
            "a {length}-byte file was accepted"
        );
    }
}
