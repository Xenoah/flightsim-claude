//! Optional bounded wall-frame diagnostics. These are not GPU timestamp queries
//! or a hardware-GPU performance claim; CPU preparation is measured separately.
//! Loading includes unresolved desired cuts and both sides of active-work
//! transitions. Steady requires two consecutive ready, idle update observations.
use super::*;
use std::collections::VecDeque;
const MAX_SAMPLES: usize = 2_048;

#[derive(Resource, Debug, Default)]
pub(super) struct RenderMetrics {
    pub enabled: bool,
    loading: VecDeque<f64>,
    steady: VecDeque<f64>,
    last_report: f64,
    // Time<Real>::delta covers the preceding app interval. Keep both sides of
    // a loading transition out of steady, including a same-update commit.
    previous_update_loading: bool,
    terrain_work_this_update: bool,
}
impl RenderMetrics {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Default::default()
        }
    }

    /// Consume existing work metadata only; never traverse a terrain cut or
    /// change streaming. A completed one-update preparation is still work.
    pub(super) fn observe_terrain_update(
        &mut self,
        was_stitching: bool,
        update: Option<&flightsim_render::TerrainUpdate>,
        progress: &flightsim_render::terrain_stitching::StitchProgress,
    ) {
        if !self.enabled {
            return;
        }
        self.terrain_work_this_update |= was_stitching
            || progress.planning_work != 0
            || progress.prepared != 0
            || progress.planning_pending
            || progress.remaining != 0
            || progress.overlays_pending
            || progress.overlay_work.copy_attempts != 0
            || progress.overlay_work.upload_attempts != 0
            || update.is_some_and(|update| {
                update.load_attempts != 0
                    || !update.prepared.is_empty()
                    || !update.spawned.is_empty()
                    || !update.hidden.is_empty()
                    || !update.despawned.is_empty()
                    || !update.replaced.is_empty()
                    || update.capacity_limited
            });
    }

    fn sample_interval(&mut self, elapsed: f64, milliseconds: f64, pending: bool) -> bool {
        if !self.enabled {
            return false;
        }
        let current_loading = pending || self.terrain_work_this_update;
        self.terrain_work_this_update = false;
        let interval_loading = self.previous_update_loading || current_loading;
        self.previous_update_loading = current_loading;
        // Preserve the startup exclusion, but observe its final state so the
        // first retained interval cannot lose an in-flight loading transition.
        if elapsed < 5.0 {
            return false;
        }
        let bucket = if interval_loading {
            &mut self.loading
        } else {
            &mut self.steady
        };
        push(bucket, milliseconds);
        true
    }
}

fn terrain_expected(startup: &Startup) -> bool {
    startup.world.global_terrain || startup.tiles.is_some()
}

/// The selector caches exact ID equality, not just equal counts. While a cut
/// is pending the displayed IDs can lag its live IDs; after atomic commit the
/// displayed cut is that selector cut. These flags therefore need no new
/// per-frame full-cut traversal. Unavailable desired data remain unresolved.
fn loading_pending(
    selector_matches_desired: bool,
    terrain_stitching: bool,
    overlay_pending: bool,
    scenery_pending: bool,
    expected_terrain_missing: bool,
) -> bool {
    !selector_matches_desired
        || terrain_stitching
        || overlay_pending
        || scenery_pending
        || expected_terrain_missing
}

#[derive(Debug, Clone, Copy)]
struct Summary {
    count: usize,
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    maximum: f64,
}
fn summary(values: &VecDeque<f64>) -> Option<Summary> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<_> = values.iter().copied().collect();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    let n = u32::try_from(count).expect("bounded samples");
    let percentile = |p: usize| sorted[(count * p).div_ceil(100).saturating_sub(1)];
    Some(Summary {
        count,
        mean: sorted.iter().sum::<f64>() / f64::from(n),
        p50: percentile(50),
        p95: percentile(95),
        p99: percentile(99),
        maximum: sorted[count - 1],
    })
}
fn push(values: &mut VecDeque<f64>, milliseconds: f64) {
    if !milliseconds.is_finite() || milliseconds <= 0.0 {
        return;
    }
    if values.len() == MAX_SAMPLES {
        values.pop_front();
    }
    values.push_back(milliseconds);
}
pub(super) fn record(
    time: Res<Time<Real>>,
    startup: Res<Startup>,
    tiles: Res<TerrainTiles>,
    streaming: Res<TerrainStreaming>,
    scenery: Res<scenery_runtime::SceneryRuntime>,
    mut metrics: ResMut<RenderMetrics>,
) {
    if !metrics.enabled {
        return;
    }
    let overlays = tiles.overlay_usage();
    let expects_terrain = terrain_expected(&startup);
    let pending = loading_pending(
        !expects_terrain || streaming.live.matches_desired(),
        tiles.is_stitching(),
        overlays.pending || overlays.dirty || overlays.optional_swap_pending,
        scenery.is_loading(),
        expects_terrain && tiles.is_empty(),
    );
    if !metrics.sample_interval(
        time.elapsed_secs_f64(),
        time.delta_secs_f64() * 1000.0,
        pending,
    ) {
        return;
    }
    if time.elapsed_secs_f64() - metrics.last_report < 5.0 {
        return;
    }
    metrics.last_report = time.elapsed_secs_f64();
    for (phase, bucket) in [("loading", &metrics.loading), ("steady", &metrics.steady)] {
        if let Some(s) = summary(bucket) {
            info!(
                "render wall frames: phase {phase}, n {}, mean {:.3} ms, p50 {:.3} ms, p95 {:.3} ms, p99 {:.3} ms, max {:.3} ms, mean {:.2} fps; complete app frame intervals, conservative loading/transition phases, not GPU timestamps",
                s.count,
                s.mean,
                s.p50,
                s.p95,
                s.p99,
                s.maximum,
                1000.0 / s.mean
            );
        }
    }
}

/// Emit once from the terrain update itself so idle/modal frames cannot repeat
/// a stale counter snapshot. These are CPU asset-submission counts, not GPU
/// upload completion or wall-time measurements. Disabled by default.
pub(super) fn report_overlay_work(enabled: bool, work: flightsim_render::TerrainOverlayFrameWork) {
    if !enabled || (work.copy_attempts == 0 && work.upload_attempts == 0) {
        return;
    }
    info!(
        "terrain overlay frame work: {} copy attempts, {} copied vertices, {} upload attempts, {} uploaded meshes, {} uploaded vertices; {} snapshot indices charged, {} snapshot indices actually scanned; CPU asset submissions, not GPU timestamps",
        work.copy_attempts,
        work.copied_vertices,
        work.upload_attempts,
        work.uploaded_meshes,
        work.uploaded_vertices,
        work.copy_indices_charged,
        work.copy_indices_scanned,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentile_distribution_and_hard_history_bound_are_exact() {
        let mut data = VecDeque::new();
        for i in 1..=100 {
            push(&mut data, f64::from(i));
        }
        let s = summary(&data).unwrap();
        assert_eq!(s.count, 100);
        assert!((s.mean - 50.5).abs() < f64::EPSILON);
        assert!((s.p50 - 50.0).abs() < f64::EPSILON);
        assert!((s.p95 - 95.0).abs() < f64::EPSILON);
        assert!((s.p99 - 99.0).abs() < f64::EPSILON);
        for _ in 0..3000 {
            push(&mut data, 16.0);
        }
        assert_eq!(data.len(), MAX_SAMPLES);
        assert!((summary(&data).unwrap().maximum - 16.0).abs() < f64::EPSILON);
    }
    #[test]
    fn invalid_or_zero_frame_intervals_are_never_counted() {
        let mut data = VecDeque::new();
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            push(&mut data, value);
        }
        assert!(summary(&data).is_none());
    }

    #[test]
    fn selector_mismatch_is_loading_before_any_stitching_begins() {
        // The selector's cached exact-ID result stays false while hidden
        // surface preparations accumulate; stitching need not have started.
        assert!(loading_pending(false, false, false, false, false));
        assert!(!loading_pending(true, false, false, false, false));
    }

    #[test]
    fn disabled_empty_terrain_ignores_the_real_selector_mismatch() {
        let mut startup = Startup::default();
        startup.world.global_terrain = false;
        startup.tiles = None;
        let mut state = flightsim_render::TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        let selector = LodSelector::new(
            16.0,
            1_080.0,
            Degrees(60.0).to_radians(),
            0,
            Meters(20_000.0),
        );
        let update = flightsim_render::update_terrain_selection(
            &selector,
            &EmptyTileSource,
            &mut cache,
            &mut state,
            Geodetic::from_degrees(35.55, 139.78, 100.0).to_ecef(),
            8,
            &mut |_, _| panic!("empty source cannot prepare terrain"),
        );
        assert!(state.desired_len() > 0);
        assert!(!state.matches_desired());
        assert!(state.is_empty());
        assert!(!terrain_expected(&startup));
        let pending = loading_pending(
            !terrain_expected(&startup) || state.matches_desired(),
            false,
            false,
            false,
            terrain_expected(&startup) && state.is_empty(),
        );
        assert!(!pending);
        assert_eq!(update.load_attempts, 0);
        let mut metrics = RenderMetrics::new(true);
        metrics.observe_terrain_update(false, Some(&update), &Default::default());
        metrics.sample_interval(6.0, 17.0, pending);
        assert!(metrics.loading.is_empty());
        assert_eq!(metrics.steady.len(), 1);
    }

    #[test]
    fn explicit_primary_tiles_expect_terrain_even_with_global_disabled() {
        let mut startup = Startup::default();
        startup.world.global_terrain = false;
        startup.tiles = Some(PathBuf::from("test-tiles"));
        assert!(terrain_expected(&startup));
        startup.tiles = None;
        assert!(!terrain_expected(&startup));
        startup.world.global_terrain = true;
        assert!(terrain_expected(&startup));
    }

    #[test]
    fn each_pending_source_prevents_a_steady_label() {
        assert!(loading_pending(true, true, false, false, false));
        assert!(loading_pending(true, false, true, false, false));
        assert!(loading_pending(true, false, false, true, false));
        assert!(loading_pending(true, false, false, false, true));
        // A deliberately terrain-disabled empty scene has no missing-expected
        // flag. Empty alone must not force an otherwise idle scene to load.
        assert!(!loading_pending(true, false, false, false, false));
    }

    #[test]
    fn pending_to_ready_transition_interval_remains_loading() {
        let mut metrics = RenderMetrics::new(true);
        assert!(metrics.sample_interval(6.0, 10.0, false));
        assert!(metrics.sample_interval(7.0, 20.0, true));
        assert!(metrics.sample_interval(8.0, 30.0, false));
        assert!(metrics.sample_interval(9.0, 40.0, false));
        assert_eq!(metrics.loading, VecDeque::from([20.0, 30.0]));
        assert_eq!(metrics.steady, VecDeque::from([10.0, 40.0]));
    }

    #[test]
    fn a_same_update_preparation_and_commit_cannot_be_steady() {
        let mut metrics = RenderMetrics::new(true);
        let update = flightsim_render::TerrainUpdate {
            load_attempts: 1,
            prepared: vec![flightsim_world::TileId::new(0, 0, 0)],
            ..Default::default()
        };
        metrics.observe_terrain_update(false, Some(&update), &Default::default());
        // Final readiness can already be true, including an empty overlay
        // cohort; both this observation and the following interval are loading.
        metrics.sample_interval(6.0, 11.0, false);
        metrics.sample_interval(7.0, 12.0, false);
        metrics.sample_interval(8.0, 13.0, false);
        assert_eq!(metrics.loading, VecDeque::from([11.0, 12.0]));
        assert_eq!(metrics.steady, VecDeque::from([13.0]));
        assert!(!metrics.terrain_work_this_update);
    }

    #[test]
    fn finishing_a_pending_cut_and_planning_or_upload_work_are_loading() {
        use flightsim_render::terrain_stitching::StitchProgress;
        for (was_stitching, progress) in [
            (true, StitchProgress::default()),
            (
                false,
                StitchProgress {
                    planning_work: 1,
                    ..Default::default()
                },
            ),
            (
                false,
                StitchProgress {
                    prepared: 1,
                    ..Default::default()
                },
            ),
        ] {
            let mut metrics = RenderMetrics::new(true);
            metrics.observe_terrain_update(was_stitching, None, &progress);
            metrics.sample_interval(6.0, 14.0, false);
            assert_eq!(metrics.loading.len(), 1);
            assert!(metrics.steady.is_empty());
        }
    }

    #[test]
    fn overlay_only_completed_work_is_loading_without_other_progress_flags() {
        use flightsim_render::{TerrainOverlayFrameWork, terrain_stitching::StitchProgress};
        for work in [
            TerrainOverlayFrameWork {
                copy_attempts: 1,
                ..Default::default()
            },
            TerrainOverlayFrameWork {
                upload_attempts: 1,
                ..Default::default()
            },
        ] {
            let mut metrics = RenderMetrics::new(true);
            let progress = StitchProgress {
                overlay_work: work,
                ..Default::default()
            };
            metrics.observe_terrain_update(false, None, &progress);
            metrics.sample_interval(6.0, 15.0, false);
            assert_eq!(metrics.loading.len(), 1);
            assert!(metrics.steady.is_empty());
        }
    }

    #[test]
    fn scenery_completion_uses_the_previous_pending_observation() {
        let mut metrics = RenderMetrics::new(true);
        // Scenery starts its task at the end of one stream update and polls
        // only on later updates. Admission likewise leaves an overlay swap
        // pending until a later terrain commit. record runs after that stream.
        metrics.sample_interval(6.0, 31.0, loading_pending(true, false, false, true, false));
        metrics.sample_interval(7.0, 32.0, loading_pending(true, false, false, false, false));
        metrics.sample_interval(8.0, 33.0, loading_pending(true, false, false, false, false));
        assert_eq!(metrics.loading, VecDeque::from([31.0, 32.0]));
        assert_eq!(metrics.steady, VecDeque::from([33.0]));
    }

    #[test]
    fn warmup_excludes_samples_but_preserves_transition_state() {
        let mut metrics = RenderMetrics::new(true);
        assert!(!metrics.sample_interval(4.9, 21.0, true));
        assert!(metrics.loading.is_empty() && metrics.steady.is_empty());
        assert!(metrics.sample_interval(5.0, 22.0, false));
        assert!(metrics.sample_interval(6.0, 23.0, false));
        assert_eq!(metrics.loading, VecDeque::from([22.0]));
        assert_eq!(metrics.steady, VecDeque::from([23.0]));
    }

    #[test]
    fn disabled_diagnostics_do_not_latch_work_or_record_samples() {
        let mut metrics = RenderMetrics::new(false);
        metrics.observe_terrain_update(true, None, &Default::default());
        assert!(!metrics.sample_interval(10.0, 50.0, true));
        assert!(!metrics.terrain_work_this_update);
        assert!(!metrics.previous_update_loading);
        assert!(metrics.loading.is_empty() && metrics.steady.is_empty());
    }

    #[test]
    fn phase_histories_remain_independently_bounded_and_ignore_invalid_intervals() {
        let mut metrics = RenderMetrics::new(true);
        for _ in 0..MAX_SAMPLES + 3 {
            metrics.sample_interval(10.0, 16.0, false);
        }
        for _ in 0..MAX_SAMPLES + 3 {
            metrics.sample_interval(10.0, 32.0, true);
        }
        assert_eq!(metrics.steady.len(), MAX_SAMPLES);
        assert_eq!(metrics.loading.len(), MAX_SAMPLES);
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            metrics.sample_interval(10.0, invalid, true);
        }
        assert_eq!(metrics.loading.len(), MAX_SAMPLES);
        assert!(
            metrics
                .loading
                .iter()
                .all(|value| (*value - 32.0).abs() < f64::EPSILON)
        );
    }
}
