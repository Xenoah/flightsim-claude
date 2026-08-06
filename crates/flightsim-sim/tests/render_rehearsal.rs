//! M2（描画）の予行演習。Bevy 抜きで、描画フレームがやることをなぞる。
//!
//! # なぜ描画の前にやるのか
//!
//! `FloatingOrigin` / `LodSelector` / `StreamingScheduler` は、いずれも
//! **自クレートの単体テスト以外で一度も動いていない**。M2 の中核でありながら
//! 実戦投入されていない状態で、そこに Bevy を載せると、下層の欠陥が
//! 「描画のバグ」に化けて切り分けが極めて難しくなる。
//!
//! ここでは実際の飛行軌跡をカメラ軌道として使い、毎フレーム
//!
//! ```text
//!   1. カメラ位置から LOD を選ぶ
//!   2. 未キャッシュのタイルを予算内で読み込み要求する
//!   3. floating origin を適用して f32 描画座標を作る（必要なら打ち直す）
//! ```
//!
//! を回して不変条件を検査する。Bevy 層はこの手順をそのまま実装すればよい。

use flightsim_core::{Degrees, Ecef, FloatingOrigin, Geodetic, Meters, Seconds};
use flightsim_fdm::AircraftConfig;
use flightsim_sim::{CircuitPlan, GroundSampler, SimulationOptions, fly};
use flightsim_world::lod::distance_to_bounds;
use flightsim_world::{LodSelector, MemoryTileSource, StreamingScheduler, Terrain, TileCache};

/// 実際の飛行軌跡をカメラ軌道として使う。
fn camera_track() -> Vec<Ecef> {
    let start = Geodetic::from_degrees(35.553, 139.781, 0.0);
    let trajectory = fly(
        &AircraftConfig::light_single(),
        &CircuitPlan::default(),
        start,
        &mut Terrain::new(MemoryTileSource::new(), 8 * 1024 * 1024, 8..=12),
        &GroundSampler::default(),
        &SimulationOptions {
            max_duration: Seconds(400.0),
            sample_interval: Seconds(2.0),
            ..SimulationOptions::default()
        },
    );
    assert!(!trajectory.diverged, "the camera track diverged");
    trajectory
        .samples
        .iter()
        .map(|sample| sample.position.to_ecef())
        .collect()
}

fn selector() -> LodSelector {
    LodSelector::new(
        16.0,
        1_080.0,
        Degrees(60.0).to_radians(),
        12,
        Meters(20_000.0),
    )
}

// --- floating origin ---

#[test]
fn the_render_transform_round_trips_within_a_millimetre() {
    // ADR-0002 の主張そのもの。ここが緩いと地表で機体が振動する。
    let track = camera_track();
    let mut origin = FloatingOrigin::new(track[0]);

    let mut worst = 0.0_f64;
    for &camera in &track {
        // 戻り値（打ち直しの有無）はここでは使わない。目的はアンカーの追従。
        let _ = origin.rebase_if_needed(camera);

        // カメラ自身と、その周囲に散らした点を往復させる。
        for offset in [
            (0.0, 0.0, 0.0),
            (100.0, -50.0, 25.0),
            (-1_500.0, 900.0, -300.0),
            (3_000.0, 3_000.0, 1_000.0),
        ] {
            let point = Ecef::new(
                camera.as_vec().x + offset.0,
                camera.as_vec().y + offset.1,
                camera.as_vec().z + offset.2,
            );
            let rendered = origin.to_render(point);
            assert!(
                rendered.is_finite(),
                "render position {rendered:?} was not finite"
            );

            let back = origin.to_world(rendered);
            let error = back.distance_to(point).get();
            worst = worst.max(error);
        }
    }

    assert!(
        worst < 0.001,
        "the worst world → render → world error was {worst:.6} m; \
         ADR-0002 claims sub-millimetre inside the rebase threshold"
    );
}

#[test]
fn relative_geometry_survives_a_rebase() {
    // 打ち直しの前後で見かけの位置関係が変わると、その瞬間に世界が飛ぶ。
    let track = camera_track();
    let mut origin = FloatingOrigin::new(track[0]);

    let landmarks: Vec<Ecef> = [(0.0, 0.0, 0.0), (500.0, 0.0, 0.0), (0.0, -800.0, 200.0)]
        .iter()
        .map(|(x, y, z)| {
            Ecef::new(
                track[0].as_vec().x + x,
                track[0].as_vec().y + y,
                track[0].as_vec().z + z,
            )
        })
        .collect();

    let mut rebases = 0_u32;
    for &camera in &track {
        let before: Vec<_> = landmarks.iter().map(|&p| origin.to_render(p)).collect();
        if origin.rebase_if_needed(camera).is_some() {
            rebases += 1;
            let after: Vec<_> = landmarks.iter().map(|&p| origin.to_render(p)).collect();

            // 個々の座標は変わってよい。変わってはいけないのは相互の距離。
            for i in 0..landmarks.len() {
                for j in (i + 1)..landmarks.len() {
                    let was = (before[i] - before[j]).length();
                    let now = (after[i] - after[j]).length();
                    assert!(
                        (f64::from(was) - f64::from(now)).abs() < 0.01,
                        "a rebase changed the distance between landmarks {i} and {j} \
                         from {was} to {now}"
                    );
                }
            }
        }
    }

    assert!(
        rebases > 0,
        "the camera never travelled far enough to trigger a rebase; \
         this test is not exercising anything"
    );
}

#[test]
fn the_anchor_never_drifts_far_from_the_camera() {
    // 打ち直しが効いていないと、飛ぶほど精度が落ちていく。
    let track = camera_track();
    let mut origin = FloatingOrigin::new(track[0]);
    let threshold = origin.rebase_threshold().get();

    for &camera in &track {
        let _ = origin.rebase_if_needed(camera);
        let distance = origin.distance_from_anchor(camera).get();
        assert!(
            distance <= threshold + 1e-6,
            "the camera drifted {distance:.1} m from the anchor, \
             beyond the {threshold:.0} m rebase threshold"
        );
    }
}

#[test]
fn a_rebase_only_happens_when_it_is_needed() {
    // 毎フレーム打ち直すと、f32 側の全オブジェクト更新が毎フレーム走る。
    let track = camera_track();
    let mut origin = FloatingOrigin::new(track[0]);

    let rebases = track
        .iter()
        .filter(|&&camera| origin.rebase_if_needed(camera).is_some())
        .count();

    assert!(
        rebases < track.len() / 4,
        "{rebases} rebases over {} frames is far too many",
        track.len()
    );
}

// --- LOD ---

#[test]
fn lod_selection_stays_within_a_tractable_tile_count() {
    // 1 フレームで数万タイルが選ばれると、描画側は何をしても間に合わない。
    let track = camera_track();
    let selector = selector();

    let mut worst = 0_usize;
    for &camera in &track {
        let selection = selector.select(camera);
        worst = worst.max(selection.tiles.len());

        assert!(
            !selection.tiles.is_empty(),
            "the LOD selector returned nothing to draw"
        );
        // 重複したタイルを 2 回描くのは純粋な無駄。
        let mut unique = selection.tiles.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            selection.tiles.len(),
            "the LOD selection contained duplicate tiles"
        );
    }

    assert!(
        worst < 5_000,
        "the worst frame selected {worst} tiles; that is not a drawable budget"
    );
}

#[test]
fn flying_higher_selects_coarser_tiles() {
    // 高度が上がるほど細かく分割されるようなら、LOD が逆に働いている。
    let selector = selector();
    let ground = Geodetic::from_degrees(35.553, 139.781, 100.0).to_ecef();
    let cruise = Geodetic::from_degrees(35.553, 139.781, 10_000.0).to_ecef();

    let deepest = |camera: Ecef| {
        selector
            .select(camera)
            .tiles
            .iter()
            .map(|tile| tile.level)
            .max()
            .expect("the selection is never empty")
    };

    assert!(
        deepest(cruise) <= deepest(ground),
        "at 10 km the selector went to level {} but at 100 m only to level {}",
        deepest(cruise),
        deepest(ground)
    );
}

// --- ストリーミング ---

#[test]
fn the_streaming_budget_is_never_exceeded_over_a_whole_flight() {
    // ここを無制限にすると、実機で必ずスタッターになる。
    let track = camera_track();
    let selector = selector();
    let budget = 8;

    let mut cache = TileCache::new(256 * 1024 * 1024);
    let mut scheduler = StreamingScheduler::new(budget);
    let mut loaded = 0_u64;

    for &camera in &track {
        let selection = selector.select(camera);
        let camera_geodetic = camera.to_geodetic();

        for tile in &selection.tiles {
            if !cache.contains(*tile) {
                scheduler.request(
                    *tile,
                    distance_to_bounds(camera, camera_geodetic, tile.bounds()),
                );
            }
        }

        let batch = scheduler.take_batch();
        assert!(
            batch.len() <= budget,
            "a frame asked for {} loads with a budget of {budget}",
            batch.len()
        );

        // 実際に読み込んだことにしてキャッシュへ入れる。
        for tile in batch {
            cache.insert(
                tile,
                flightsim_world::DemTile::new(
                    tile.bounds(),
                    flightsim_world::HeightGrid::flat(33, 33, Meters(100.0)),
                ),
            );
            loaded += 1;
        }

        assert!(
            cache.used_bytes() <= cache.capacity_bytes(),
            "the cache exceeded its byte budget"
        );
    }

    assert!(loaded > 0, "nothing was ever streamed in");
}

#[test]
fn the_scheduler_drains_rather_than_growing_without_bound() {
    // カメラが止まれば、要求は有限回で捌けきるはず。
    // 捌けないなら、実運用ではキューが際限なく伸びる。
    let selector = selector();
    let camera = Geodetic::from_degrees(35.553, 139.781, 2_000.0).to_ecef();
    let camera_geodetic = camera.to_geodetic();
    let selection = selector.select(camera);

    let mut cache = TileCache::new(256 * 1024 * 1024);
    let mut scheduler = StreamingScheduler::new(8);
    for tile in &selection.tiles {
        scheduler.request(
            *tile,
            distance_to_bounds(camera, camera_geodetic, tile.bounds()),
        );
    }

    let mut frames = 0_u32;
    while scheduler.pending() > 0 {
        for tile in scheduler.take_batch() {
            cache.insert(
                tile,
                flightsim_world::DemTile::new(
                    tile.bounds(),
                    flightsim_world::HeightGrid::flat(9, 9, Meters(0.0)),
                ),
            );
        }
        frames += 1;
        assert!(
            frames < 10_000,
            "the queue still had {} entries after {frames} frames",
            scheduler.pending()
        );
    }

    assert_eq!(cache.len(), selection.tiles.len());
}

// --- 全部つないだフレームループ ---

#[test]
fn a_full_frame_loop_holds_all_of_its_invariants() {
    // M2 の描画フレームがやることを一通り回す。
    let track = camera_track();
    let selector = selector();

    let mut origin = FloatingOrigin::new(track[0]);
    let mut cache = TileCache::new(128 * 1024 * 1024);
    let mut scheduler = StreamingScheduler::new(8);

    for (frame, &camera) in track.iter().enumerate() {
        // 1. LOD
        let selection = selector.select(camera);
        let camera_geodetic = camera.to_geodetic();

        // 2. ストリーミング（予算制）
        for tile in &selection.tiles {
            if !cache.contains(*tile) {
                scheduler.request(
                    *tile,
                    distance_to_bounds(camera, camera_geodetic, tile.bounds()),
                );
            }
        }
        for tile in scheduler.take_batch() {
            cache.insert(
                tile,
                flightsim_world::DemTile::new(
                    tile.bounds(),
                    flightsim_world::HeightGrid::flat(17, 17, Meters(50.0)),
                ),
            );
        }

        // 3. floating origin
        let _ = origin.rebase_if_needed(camera);

        // 描画対象のタイル中心を f32 へ落とす。
        for tile in selection.tiles.iter().take(64) {
            let centre = tile.center().to_ecef();
            let rendered = origin.to_render(centre);
            assert!(
                rendered.is_finite(),
                "frame {frame}: tile {tile:?} rendered to {rendered:?}"
            );
            // f32 の指数レンジを超えると Inf になる。地球規模でも起きないこと。
            assert!(
                rendered.length() < 1.0e8,
                "frame {frame}: tile {tile:?} landed {} m from the anchor",
                rendered.length()
            );
        }

        assert!(
            origin.distance_from_anchor(camera).get() <= origin.rebase_threshold().get() + 1e-6
        );
        assert!(cache.used_bytes() <= cache.capacity_bytes());
    }
}
