//! 敵対的 PBF に対する `airportgen` の境界の検査。
//!
//! # 何を確かめるか
//!
//! **細工・破損した PBF で panic せず、通常のエラーとして報告すること。**
//!
//! `osmpbf 0.3.7` は malformed protobuf の極端な delta / offset を
//! unchecked な i64 算術で復号する箇所があり、debug で panic、
//! release で wrap しうる（Issue #23）。正規提供元の PBF では起きないが、
//! **利用者が拾ってきた PBF は untrusted 入力**である。
//!
//! ここでは実際に壊したバイト列を投げて、**プロセスが落ちない**ことだけを
//! 見る。何をエラーと呼ぶかは実装の裁量。

use flightsim_tilegen::airport::generate_airport_database;

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
    let outcome = generate_airport_database(&path, &output)
        .map(|_| ())
        .map_err(|error| error.to_string());
    std::fs::remove_dir_all(&directory).ok();
    outcome
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
