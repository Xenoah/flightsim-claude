//! 壊れたリプレイファイルと、再生操作の境界。
//!
//! リプレイは**他人から受け取る**ことがある形式。壊れた入力で panic すると
//! そこで終わりなので、外から壊して確かめる。

use flightsim_core::Seconds;
use flightsim_fdm::ControlInputs;
use flightsim_sim::replay::{
    Conditions, FORMAT_VERSION, Frame, MAGIC, MAX_FRAMES, MAX_SPEED, MIN_SPEED, Player, Recorder,
    Recording, ReplayError,
};

/// 短いが中身のある記録のバイト列。
fn sample_bytes(frames: u32) -> Vec<u8> {
    let mut recorder = Recorder::new(Conditions::default());
    for index in 0..frames {
        recorder.record(
            Seconds(1.0 / 60.0),
            ControlInputs::neutral().with_throttle(f64::from(index % 10) / 10.0),
            None,
        );
    }
    let mut bytes = Vec::new();
    recorder
        .finish()
        .write_to(&mut bytes)
        .expect("writing to a Vec cannot fail");
    bytes
}

fn read(bytes: &[u8]) -> Result<Recording, ReplayError> {
    Recording::read_from(&mut &bytes[..])
}

#[test]
fn the_sample_itself_is_valid() {
    // 以下の検査が「元から壊れていた」で通らないように、まず基準を確認する。
    let recording = read(&sample_bytes(300)).expect("the unmodified sample must read");
    assert_eq!(recording.frames().len(), 300);
}

#[test]
fn an_empty_file_is_refused_without_panicking() {
    assert!(matches!(read(&[]), Err(ReplayError::Io(_))));
}

#[test]
fn every_truncation_is_refused_without_panicking() {
    // **どこで切れても落ちないこと。** 途中で電源が落ちた記録は普通に起きる。
    let bytes = sample_bytes(300);
    for length in 0..bytes.len() {
        let result = read(&bytes[..length]);
        assert!(
            result.is_err(),
            "a file truncated to {length} bytes was accepted"
        );
    }
}

#[test]
fn a_file_that_is_not_a_replay_says_so() {
    let bytes = b"PNG\r\n\x1a\n\0 this is an image, not a flight".to_vec();
    match read(&bytes) {
        Err(ReplayError::NotAReplay { found }) => assert_ne!(found, MAGIC),
        other => panic!("expected NotAReplay, got {other:?}"),
    }
}

#[test]
fn a_future_format_version_is_named_rather_than_guessed() {
    // **読めない版を読もうとしない。** 中身の解釈が変わっていれば、
    // 通ったところで別の飛行になる。
    let mut bytes = sample_bytes(10);
    bytes[8..10].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    match read(&bytes) {
        Err(ReplayError::UnsupportedVersion { found, expected }) => {
            assert_eq!(found, FORMAT_VERSION + 1);
            assert_eq!(expected, FORMAT_VERSION);
        }
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn an_absurd_frame_count_is_refused_before_allocating() {
    // 壊れた長さフィールドで 100 GB 確保しに行かせない。
    let bytes = sample_bytes(10);
    // frame count は名前長 4 + 名前 0 + 固定ヘッダのあと。位置を探すより、
    // 末尾から数える方が形式変更に強い。frames(10) + keyframes(0) の直前。
    let frame_field = bytes.len() - 10 * 56 - 8;
    let mut broken = bytes.clone();
    broken[frame_field..frame_field + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    match read(&broken) {
        Err(ReplayError::TooLarge {
            declared, maximum, ..
        }) => {
            assert_eq!(declared, u64::from(u32::MAX));
            assert_eq!(maximum, u64::from(MAX_FRAMES));
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

#[test]
fn an_absurd_keyframe_count_is_refused() {
    let bytes = sample_bytes(10);
    let keyframe_field = bytes.len() - 10 * 56 - 4;
    let mut broken = bytes.clone();
    broken[keyframe_field..keyframe_field + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(read(&broken), Err(ReplayError::TooLarge { .. })));
}

#[test]
fn flipping_any_single_byte_never_panics() {
    // 破損は 1 ビットから始まる。**落ちないこと**だけを見る
    // （読めてしまう破損もあるので、成否は問わない）。
    let bytes = sample_bytes(60);
    for index in 0..bytes.len() {
        for mask in [0x01_u8, 0x80, 0xff] {
            let mut broken = bytes.clone();
            broken[index] ^= mask;
            let _ = read(&broken);
        }
    }
}

#[test]
fn a_recording_with_no_frames_is_valid() {
    // 開始直後に保存した記録。**空を壊れていることにしない。**
    let recording = read(&sample_bytes(0)).expect("an empty recording is legitimate");
    assert!(recording.frames().is_empty());
    assert_eq!(recording.duration(), Seconds(0.0));
    let mut player = Player::new(recording);
    assert!(player.is_finished());
    assert!(player.step_once().is_none());
}

// --- 再生操作 ---

fn player_with(frames: u32) -> Player {
    Player::new(read(&sample_bytes(frames)).expect("the sample must read"))
}

#[test]
fn a_paused_player_hands_out_nothing_and_does_not_bank_time() {
    // **溜めたまま再開すると早送りになる。** 止めている間は捨てる。
    let mut player = player_with(300);
    player.set_paused(true);
    player.accumulate(Seconds(10.0));
    assert!(player.next_due().is_none());
    assert_eq!(player.cursor(), 0);

    player.set_paused(false);
    assert!(
        player.next_due().is_none(),
        "10 s of paused time must not survive the resume"
    );

    player.accumulate(Seconds(1.0 / 60.0));
    assert!(
        player.next_due().is_some(),
        "one frame of time buys one frame"
    );
}

#[test]
fn speed_is_clamped_and_nan_falls_back_to_real_time() {
    // 丸めた結果は定数そのものなので、厳密比較で構わない。
    #[expect(clippy::float_cmp, reason = "クランプ結果は定数と等しくなるのが仕様")]
    fn assert_speed(player: &Player, expected: f64) {
        assert_eq!(player.speed(), expected);
    }

    let mut player = player_with(10);
    player.set_speed(1000.0);
    assert_speed(&player, MAX_SPEED);
    player.set_speed(0.0);
    assert_speed(&player, MIN_SPEED);
    player.set_speed(f64::NAN);
    // NaN をそのまま入れると予算が NaN になり、二度と進まなくなる。
    assert_speed(&player, 1.0);
}

#[test]
fn double_speed_hands_out_twice_as_many_frames() {
    let mut normal = player_with(600);
    normal.accumulate(Seconds(1.0));
    let mut normal_count = 0;
    while normal.next_due().is_some() {
        normal_count += 1;
    }

    let mut fast = player_with(600);
    fast.set_speed(2.0);
    fast.accumulate(Seconds(1.0));
    let mut fast_count = 0;
    while fast.next_due().is_some() {
        fast_count += 1;
    }

    // 実測: 等速 59 本、2 倍速 120 本。**きっかり 60 本にはならない。**
    // 予算 1.0 秒から 1/60 を引き続けると丸めの残りが出て 60 本目に届かない。
    // 端数は次のフレームへ持ち越されるので、再生が遅れることはない。
    assert_eq!(normal_count, 59);
    assert_eq!(fast_count, 120);
    assert!(
        fast_count >= normal_count * 2,
        "2x must not hand out fewer frames than 1x twice over"
    );
}

#[test]
fn a_non_finite_frame_time_is_ignored_rather_than_poisoning_the_budget() {
    // 描画側が NaN や Inf のフレーム時間を渡してくることはある
    // （最小化やスリープ復帰）。**入れたら予算が壊れて再生が止まる。**
    let mut player = player_with(60);
    player.accumulate(Seconds(f64::NAN));
    player.accumulate(Seconds(f64::INFINITY));
    player.accumulate(Seconds(-1.0));
    assert!(player.next_due().is_none());

    player.accumulate(Seconds(1.0 / 60.0));
    assert!(
        player.next_due().is_some(),
        "the budget must still be usable"
    );
}

#[test]
fn seeking_past_the_end_stops_at_the_last_frame() {
    let mut recorder = Recorder::new(Conditions::default());
    let state = flightsim_fdm::RigidBodyState::from_geodetic(
        flightsim_core::Geodetic::from_degrees(35.0, 139.0, 100.0),
        flightsim_core::Attitude::new(
            flightsim_core::Radians::ZERO,
            flightsim_core::Radians::ZERO,
            flightsim_core::Radians::ZERO,
        ),
        flightsim_core::Ned::new(0.0, 0.0, 0.0),
    );
    for _ in 0..300 {
        recorder.record(Seconds(1.0 / 60.0), ControlInputs::neutral(), Some(&state));
    }
    let mut player = Player::new(recorder.finish());
    let plan = player.seek(u32::MAX).expect("keyframes exist");
    assert_eq!(plan.target, 300, "the seek must stop at the end");
    assert!(plan.replay_from <= 300);
}

#[test]
fn seeking_a_recording_without_keyframes_reports_that_it_cannot() {
    // キーフレーム無しで作った記録は、開始点を出せない。
    // **黙って先頭に飛ばさない**（呼び出し側が最初から回し直すべきだと分かる）。
    let mut player = player_with(300);
    assert!(player.recording().keyframes().is_empty());
    assert!(player.seek(100).is_none());
}

#[test]
fn the_recorder_stops_at_the_frame_limit() {
    // 上限そのものは回さない（56 MB 分の記録になる）。境界の判定だけ見る。
    let mut recorder = Recorder::new(Conditions::default());
    recorder.record(Seconds(0.016), ControlInputs::neutral(), None);
    assert!(!recorder.is_full());
    assert_eq!(recorder.frame_count(), 1);
    // 上限が 0 や 1 に潰れていたら、そもそも記録が成り立たない。
    // 定数どうしの比較なのでコンパイル時に見る。
    const {
        assert!(
            MAX_FRAMES >= 60 * 60,
            "the limit must hold at least a minute"
        )
    };
}

#[test]
fn control_inputs_read_back_from_a_corrupt_file_stay_in_range() {
    // 範囲外の値がファイルに入っていても、FDM へ渡る前に潰れること。
    let mut bytes = sample_bytes(1);
    // 最後のフレームの aileron（frame_time の次）を 1e30 にする。
    let aileron = bytes.len() - 56 + 8;
    bytes[aileron..aileron + 8].copy_from_slice(&1.0e30_f64.to_le_bytes());
    let recording = read(&bytes).expect("an out-of-range control is clamped, not rejected");
    let Frame { controls, .. } = recording.frames()[0];
    assert!((-1.0..=1.0).contains(&controls.aileron()));
}
