//! Optional bounded wall-frame diagnostics. These are not GPU timestamp queries
//! or a hardware-GPU performance claim; CPU preparation is measured separately.
use super::*;
use std::collections::VecDeque;
const MAX_SAMPLES: usize = 2_048;

#[derive(Resource, Debug, Default)]
pub(super) struct RenderMetrics {
    pub enabled: bool,
    loading: VecDeque<f64>,
    steady: VecDeque<f64>,
    last_report: f64,
}
impl RenderMetrics {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Default::default()
        }
    }
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
    scenery: Res<scenery_runtime::SceneryRuntime>,
    mut metrics: ResMut<RenderMetrics>,
) {
    if !metrics.enabled || time.elapsed_secs_f64() < 5.0 {
        return;
    }
    let loading = tiles.is_stitching()
        || scenery.is_loading()
        || (startup.world.global_terrain && tiles.is_empty());
    let bucket = if loading {
        &mut metrics.loading
    } else {
        &mut metrics.steady
    };
    push(bucket, time.delta_secs_f64() * 1000.0);
    if time.elapsed_secs_f64() - metrics.last_report < 5.0 {
        return;
    }
    metrics.last_report = time.elapsed_secs_f64();
    for (phase, bucket) in [("loading", &metrics.loading), ("steady", &metrics.steady)] {
        if let Some(s) = summary(bucket) {
            info!(
                "render wall frames: phase {phase}, n {}, mean {:.3} ms, p50 {:.3} ms, p95 {:.3} ms, p99 {:.3} ms, max {:.3} ms, mean {:.2} fps; complete app frame intervals, not GPU timestamps",
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
        "terrain overlay frame work: {} copy attempts, {} copied vertices, {} upload attempts, {} uploaded meshes, {} uploaded vertices; CPU asset submissions, not GPU timestamps",
        work.copy_attempts,
        work.copied_vertices,
        work.upload_attempts,
        work.uploaded_meshes,
        work.uploaded_vertices,
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
}
