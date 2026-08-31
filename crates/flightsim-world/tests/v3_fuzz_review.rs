//! FSAP v3 の破損入力に対する独立検算。
//!
//! # なぜ二重に検査するのか
//!
//! `io/v3.rs` の中にも拒否の検査がある。ここはそれとは別に、
//! **レビュー側が外から確かめた記録**として残す。実装と同じ人が書いた
//! テストだけだと、前提の取り違えが両方に同じ形で入る。
//!
//! ここで確かめるのは 1 点だけ:
//! **どんなバイト列を投げても panic せず、`Err` か正しい値を返すこと。**
//! 壊れたファイルでプロセスが落ちると、利用者は原因を掴めない。

use flightsim_world::AirportDatabase;
use std::io::Cursor;

/// 実際に書き出した正当な v3 を土台にする。
///
/// 合成のバイト列だけを投げると「ヘッダで弾かれて終わり」になり、
/// section directory から先の経路を通らない。
fn valid_v3() -> Vec<u8> {
    let database = AirportDatabase::new(Vec::new()).expect("an empty database is valid");
    let mut bytes = Vec::new();
    database
        .write_to(&mut bytes)
        .expect("an empty database must be writable");
    bytes
}

fn read(bytes: &[u8]) -> Result<AirportDatabase, impl std::fmt::Debug> {
    AirportDatabase::read_from(&mut Cursor::new(bytes))
}

#[test]
fn a_valid_database_round_trips() {
    // 土台が正しいことを先に確かめる。ここが壊れていると、
    // 以下の検査は「何を壊しても Err」になって意味を失う。
    let bytes = valid_v3();
    assert!(
        read(&bytes).is_ok(),
        "the baseline database should read back"
    );
}

#[test]
fn every_truncation_is_rejected_without_panicking() {
    // 途中で切れたファイル。ダウンロード失敗やディスク full で普通に起きる。
    let bytes = valid_v3();
    for length in 0..bytes.len() {
        let result = read(&bytes[..length]);
        assert!(
            result.is_err(),
            "a database truncated to {length} bytes was accepted"
        );
    }
}

#[test]
fn every_single_byte_flip_is_rejected_or_read_safely() {
    // 1 バイトの破損。checksum があるので大半は Err になるはずだが、
    // **要件は「panic しないこと」**であって「必ず Err」ではない。
    let bytes = valid_v3();
    for index in 0..bytes.len() {
        for mask in [0x01_u8, 0x80, 0xFF] {
            let mut corrupted = bytes.clone();
            corrupted[index] ^= mask;
            // Ok でも Err でもよい。panic しないことだけを見る。
            let _ = read(&corrupted);
        }
    }
}

#[test]
fn absurd_section_counts_do_not_allocate_wildly() {
    // **宣言された件数を信じて先に確保すると、小さなファイルで
    // メモリを食い尽くせる（zip bomb）。** 32 bit 全域の件数を宣言する。
    let mut bytes = valid_v3();
    assert!(bytes.len() >= 16, "the header should be at least 16 bytes");

    // header の「section 数」を最大にする。位置は ADR の表による
    // （offset 8、u32）。
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let result = read(&bytes);
    assert!(
        result.is_err(),
        "a database claiming u32::MAX sections was accepted"
    );
}

#[test]
fn absurd_record_sizes_are_rejected() {
    // directory entry size を最大にする（offset 12、u32）。
    let mut bytes = valid_v3();
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        read(&bytes).is_err(),
        "a database claiming u32::MAX entry size was accepted"
    );
}

#[test]
fn random_bytes_never_panic() {
    // 決定論的な擬似乱数で作った列。**乱数を引かない**（再現できないと
    // 落ちたときに追えない）。
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for length in [0_usize, 1, 7, 24, 25, 64, 256, 4096] {
        for _ in 0..40 {
            let bytes: Vec<u8> = (0..length).map(|_| (next() & 0xFF) as u8).collect();
            let _ = read(&bytes);
        }
    }
}

#[test]
fn a_header_with_an_unknown_format_version_is_rejected() {
    // 将来の版を黙って読むと、意味の違う byte を誤解釈する。
    let mut bytes = valid_v3();
    // format version は offset 4（u16）。
    bytes[4..6].copy_from_slice(&999_u16.to_le_bytes());
    assert!(
        read(&bytes).is_err(),
        "an unknown format version was accepted"
    );
}
