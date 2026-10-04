//! Independent upper water controls and observable fallback/preparation state.

use bevy::prelude::*;
use flightsim_render::water::{WaterDiagnostics, WaterQuality};

use crate::{Startup, world_runtime};

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        select_water_quality
            .after(world_runtime::capture_map_input)
            .before(super::publish_hud)
            .run_if(world_runtime::flight_controls_active),
    )
    .add_systems(Update, log_water_status);
}

fn select_water_quality(keyboard: Res<ButtonInput<KeyCode>>, mut quality: ResMut<WaterQuality>) {
    if keyboard.just_pressed(KeyCode::F1) {
        *quality = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
        {
            WaterQuality::Light
        } else {
            quality.next()
        };
    }
}

pub(super) fn quality_label(quality: WaterQuality, status: &WaterDiagnostics) -> &'static str {
    let ready = status.ready_pairs > 0 && status.fallback_pairs == 0 && !status.failed_pipelines;
    match quality {
        WaterQuality::Light => "LIGHT",
        WaterQuality::High if ready => "HIGH",
        WaterQuality::Ultra if ready => "ULTRA",
        WaterQuality::High => "HIGH (LIGHT)",
        WaterQuality::Ultra => "ULTRA (LIGHT)",
    }
}

#[derive(Debug, Clone, Copy)]
struct WaterLogEntry {
    at: f64,
    quality: WaterQuality,
    preparing: bool,
    ready: bool,
    failed: bool,
}

fn log_water_status(
    startup: Option<Res<Startup>>,
    quality: Res<WaterQuality>,
    status: Res<WaterDiagnostics>,
    time: Option<Res<Time<Real>>>,
    mut last: Local<Option<WaterLogEntry>>,
) {
    let (Some(startup), Some(time)) = (startup, time) else {
        return;
    };
    if !startup.render_stats {
        return;
    }
    let ready = *quality != WaterQuality::Light
        && status.ready_pairs > 0
        && status.fallback_pairs == 0
        && !status.failed_pipelines;
    let now = time.elapsed_secs_f64();
    if last.is_some_and(|previous| {
        now - previous.at < 5.0
            && previous.quality == *quality
            && previous.preparing == status.preparing_mask
            && previous.ready == ready
            && previous.failed == status.failed_pipelines
    }) {
        return;
    }
    *last = Some(WaterLogEntry {
        at: now,
        quality: *quality,
        preparing: status.preparing_mask,
        ready,
        failed: status.failed_pipelines,
    });
    info!(
        "water stats requested={} ready={} preparing={} mask_bytes={} completed_texels={} proxies={} materials={} ready_pairs={} fallback_pairs={} failed={}",
        quality.name(),
        ready,
        status.preparing_mask,
        status.mask_bytes,
        status.mask_completed_texels,
        status.proxy_count,
        status.material_count,
        status.ready_pairs,
        status.fallback_pairs,
        status.failed_pipelines,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_cli_is_independent_and_invalid_names_fail() {
        for quality in [WaterQuality::Light, WaterQuality::High, WaterQuality::Ultra] {
            let (startup, _) = crate::parse_arguments_from([
                "--water-quality".to_owned(),
                quality.name().to_ascii_lowercase(),
            ]);
            assert_eq!(startup.water_quality, quality);
            assert_eq!(startup.cloud_quality, default());
            assert_eq!(startup.graphics_quality, default());
            assert!(startup.graphics_error.is_none());
        }
        for value in ["", "HIGH", "off", "--wind"] {
            let (startup, _) =
                crate::parse_arguments_from(["--water-quality", value].map(str::to_owned));
            assert!(startup.graphics_error.is_some());
        }
    }

    #[test]
    fn pending_or_failed_water_does_not_claim_upper_readiness() {
        let mut status = WaterDiagnostics::default();
        assert_eq!(quality_label(WaterQuality::High, &status), "HIGH (LIGHT)");
        status.ready_pairs = 1;
        assert_eq!(quality_label(WaterQuality::High, &status), "HIGH");
        status.fallback_pairs = 1;
        assert_eq!(quality_label(WaterQuality::Ultra, &status), "ULTRA (LIGHT)");
        status.fallback_pairs = 0;
        status.failed_pipelines = true;
        assert_eq!(quality_label(WaterQuality::High, &status), "HIGH (LIGHT)");
        assert_eq!(quality_label(WaterQuality::Light, &status), "LIGHT");
    }
}
