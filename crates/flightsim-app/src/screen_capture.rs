//! Actual-scene screenshots, including a windowless software-rendering path.
//!
//! The windowless path changes the camera output surface. Native batch capture
//! defers unfinished view draws while preserving normal preparation and budgets.
//! Physics, assets, atmosphere, terrain and UI use the interactive app systems.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use flightsim_core::Geodetic;
use flightsim_render::{CameraWorldPosition, RenderOrigin, TerrainOverlayUsage, TerrainTiles};
use flightsim_world::TileId;
use std::io::Write;
use std::path::Path;

use crate::{ActiveRunway, ExteriorModel, PendingModelFit, Startup, TerrainStreaming};

/// The active airport surface, independent of view-frustum visibility. An
/// intentionally hidden/precision-rejected runway must not pass capture readiness.
#[derive(Component)]
pub(super) struct RunwaySurface;

pub(super) fn configure(app: &mut App) {
    #[cfg(not(target_family = "wasm"))]
    crate::capture_backpressure::configure(app);
    #[cfg(not(target_family = "wasm"))]
    crate::capture_admission::configure(app);
    if app.world().resource::<Startup>().screenshot.is_some() {
        app.init_resource::<CaptureTerrainReadiness>();
    }
    // Update commits terrain/model changes through deferred Commands. Last is
    // after those commands and PostUpdate transform/visibility propagation, but
    // before the render sub-app extracts the screenshot request.
    app.add_systems(Last, capture_screenshot);
}

/// Capture-only selector observation, removed after the one-shot request.
/// It does not read sources or repeat LOD selection.
#[derive(Resource, Default)]
pub(super) struct CaptureTerrainReadiness {
    pub(super) settled: bool,
    missing_reads: usize,
    failed_reads: usize,
    capacity_limited_updates: usize,
}

impl CaptureTerrainReadiness {
    pub(super) fn observe(
        &mut self,
        update: &flightsim_render::TerrainUpdate,
        observed: Option<bool>,
    ) {
        self.settled = observed == Some(true);
        self.missing_reads = self.missing_reads.saturating_add(update.missing);
        self.failed_reads = self.failed_reads.saturating_add(update.failed);
        self.capacity_limited_updates = self
            .capacity_limited_updates
            .saturating_add(usize::from(update.capacity_limited));
    }
}

#[derive(Debug, PartialEq)]
struct ReadyScene {
    startup_revision: u32,
    origin: Geodetic,
    // Entities distinguish same-ID source replacements; IDs alone do not.
    surfaces: Vec<(TileId, Entity)>,
    overlay_revision: u64,
    precision_hidden: usize,
    runway: Option<(Entity, AssetId<Mesh>)>,
    models: Vec<Entity>,
}

#[derive(Default)]
struct CaptureState {
    elapsed: f64,
    frames: u32,
    done: bool,
    previous_ready: Option<ReadyScene>,
    scene_changed: bool,
}

impl CaptureState {
    fn observe(&mut self, ready: Option<ReadyScene>) -> bool {
        let stable = ready.is_some() && self.previous_ready == ready;
        // Reuse the existing comparison; do not traverse a ready scene twice.
        self.scene_changed = !stable && (ready.is_some() || self.previous_ready.is_some());
        self.previous_ready = ready;
        stable
    }
}

fn terrain_ready(
    availability_settled: bool,
    live_matches_desired: bool,
    stitching: bool,
    displayed: impl Iterator<Item = TileId>,
    live: impl Iterator<Item = TileId>,
    overlays: TerrainOverlayUsage,
) -> bool {
    // Coarse primary ancestors outrank fine fallback; requested levels can also
    // exceed a source's cap. Those sparse/empty availability cuts need not ever
    // equal raw desired IDs. Require examined current dependency paths in that
    // case, plus exact displayed/live IDs and completed bridge/overlay work.
    crate::displayed_cut_matches_desired(
        availability_settled || live_matches_desired,
        stitching,
        displayed,
        live,
    ) && !overlays.pending
        && !overlays.dirty
        && !overlays.optional_swap_pending
        && overlays.revision == overlays.committed_revision
}

#[derive(Resource, Debug)]
pub(super) struct OffscreenTarget(Handle<Image>);

pub(super) fn setup_offscreen_target(
    startup: Res<Startup>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<Entity, With<Camera3d>>,
) {
    if !startup.headless_screenshot {
        return;
    }
    let image = images.add(Image::new_target_texture(
        1280,
        720,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    for entity in &cameras {
        commands
            .entity(entity)
            .insert((RenderTarget::Image(image.clone().into()), IsDefaultUiCamera));
    }
    commands.insert_resource(OffscreenTarget(image));
    info!("offscreen capture: actual scene and UI at 1280x720");
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy screenshot scheduling inputs"
)]
fn capture_screenshot(
    time: Res<Time>,
    startup: Res<Startup>,
    target: Option<Res<OffscreenTarget>>,
    streaming: Res<TerrainStreaming>,
    tiles: Res<TerrainTiles>,
    origin: Res<RenderOrigin>,
    camera_position: Res<CameraWorldPosition>,
    active_runway: Option<Res<ActiveRunway>>,
    terrain_readiness: Option<Res<CaptureTerrainReadiness>>,
    runways: Query<(Entity, &Mesh3d, &InheritedVisibility), With<RunwaySurface>>,
    pending_models: Query<(), With<PendingModelFit>>,
    models: Query<Entity, With<ExteriorModel>>,
    mut commands: Commands,
    mut state: Local<CaptureState>,
    #[cfg(not(target_family = "wasm"))] mut admission: Option<
        ResMut<crate::capture_admission::CaptureAdmission>,
    >,
) {
    let Some(path) = startup.screenshot.as_ref() else {
        return;
    };
    if state.done {
        return;
    }
    state.elapsed += f64::from(time.delta_secs());
    state.frames = state.frames.saturating_add(1);
    let overlays = tiles.overlay_usage();
    // Explicit worldwide starts can retain a distant synthetic runway. Reuse
    // the app's active-airport vicinity policy, rather than waiting forever for
    // a remote runway that is correctly hidden by the precision gate.
    let runway_required = active_runway.as_ref().is_some_and(|runway| {
        crate::point_is_near_runway(camera_position.0, runway.0, crate::ACTIVE_AIRPORT_RADIUS)
    });
    let runway = runways
        .single()
        .ok()
        .filter(|(_, _, visible)| visible.get());
    let ready = (pending_models.is_empty()
        && (!runway_required || runway.is_some())
        && terrain_ready(
            terrain_readiness
                .as_ref()
                .is_some_and(|ready| ready.settled),
            streaming.live.matches_desired(),
            tiles.is_stitching(),
            tiles.displayed_ids(),
            streaming.live.ids(),
            overlays,
        ))
    .then(|| ReadyScene {
        startup_revision: startup.last_changed().get(),
        origin: origin.0.anchor(),
        surfaces: tiles
            .displayed_ids()
            .map(|id| (id, tiles.entity(id).expect("displayed terrain entity")))
            .collect(),
        overlay_revision: overlays.committed_revision,
        // Optional distant scenery may legitimately remain precision-hidden.
        // The required runway's propagated visibility is checked above instead
        // of requiring every optional asset in the world to become visible.
        precision_hidden: overlays.precision_hidden,
        runway: runway
            .filter(|_| runway_required)
            .map(|(entity, mesh, _)| (entity, mesh.0.id())),
        models: {
            let mut entities: Vec<_> = models.iter().collect();
            entities.sort_unstable();
            entities
        },
    });
    // One complete previous frame must have had this same committed CPU scene.
    // It gets an extraction/render-preparation opportunity before we request
    // capture. This is not a GPU/pipeline completion acknowledgement.
    let stable = state.observe(ready);
    #[cfg(not(target_family = "wasm"))]
    if let Some(admission) = admission.as_deref_mut() {
        let eligible = state.elapsed >= startup.screenshot_delay && state.frames >= 30 && stable;
        if !admission.observe(state.scene_changed, eligible) {
            return;
        }
    }
    if state.elapsed < startup.screenshot_delay || state.frames < 30 || !stable {
        return;
    }
    let ready = state.previous_ready.as_ref().expect("stable ready scene");
    info!(
        "screenshot CPU ready: {} displayed, {} live ({} primary, {} fallback), {} desired, live_match {}, availability_settled {}, stitching false, overlay revision {}/{}, pending false, precision hidden {}, runway {:?}, origin {:.6},{:.6}, discovery missing {}, failed {}, capacity-limited updates {}; same scene on consecutive frames, not GPU completion",
        ready.surfaces.len(),
        streaming.live.len(),
        streaming.live.len() - streaming.live.fallback_len(),
        streaming.live.fallback_len(),
        streaming.live.desired_len(),
        streaming.live.matches_desired(),
        terrain_readiness
            .as_ref()
            .is_some_and(|ready| ready.settled),
        overlays.committed_revision,
        overlays.revision,
        overlays.precision_hidden,
        ready.runway,
        ready.origin.latitude_degrees(),
        ready.origin.longitude_degrees(),
        terrain_readiness
            .as_ref()
            .map_or(0, |ready| ready.missing_reads),
        terrain_readiness
            .as_ref()
            .map_or(0, |ready| ready.failed_reads),
        terrain_readiness
            .as_ref()
            .map_or(0, |ready| ready.capacity_limited_updates),
    );
    state.done = true;
    #[cfg(not(target_family = "wasm"))]
    if let Some(admission) = admission.as_deref_mut() {
        admission.requested();
    }
    state.previous_ready = None;
    commands.remove_resource::<CaptureTerrainReadiness>();
    let screenshot = target.map_or_else(Screenshot::primary_window, |target| {
        Screenshot::image(target.0.clone())
    });
    info!("capturing a screenshot to {}", path.display());
    commands.spawn(screenshot).observe(save_capture);
    if startup.windows_readback_diagnostic {
        commands.insert_resource(crate::windows_readback_diagnostic::ProbeRequest);
        eprintln!("FS_READBACK_PROBE event=armed");
    }
}

fn save_capture(
    captured: On<ScreenshotCaptured>,
    startup: Res<Startup>,
    #[cfg(not(target_family = "wasm"))] session: Option<
        Res<crate::capture_backpressure::CaptureSession>,
    >,
    #[cfg(not(target_family = "wasm"))] admission: Option<
        ResMut<crate::capture_admission::CaptureAdmission>,
    >,
) {
    #[cfg(not(target_family = "wasm"))]
    if let Some(mut admission) = admission {
        admission.finish();
    }
    #[cfg(not(target_family = "wasm"))]
    if let Some(session) = session {
        session.finish();
    }
    let Some(path) = startup.screenshot.as_ref() else {
        return;
    };
    let result = save_capture_image(captured.image.clone(), path);
    match &result {
        Ok(()) => info!("Screenshot saved to {}", path.display()),
        Err(error) => error!("Cannot save screenshot: {error}"),
    }
    finish_batch_capture(startup.exit_after_screenshot, &result);
}

/// Encoding, final buffered writes and filesystem synchronization must all
/// succeed before a capture is acknowledged. ImageBuffer::save drops its own
/// BufWriter, which cannot report an error from the final flush.
fn save_capture_image(image: Image, path: &Path) -> Result<(), String> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return Err("screenshot path must end in .png".into());
    }
    let image = image
        .try_into_dynamic()
        .map_err(|error| error.to_string())?;
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    let format = bevy::image::ImageFormat::Png
        .as_image_crate_format()
        .expect("PNG is enabled");
    // HDR alpha represents brightness, not transparency, matching Bevy's saver.
    image
        .to_rgb8()
        .write_to(&mut writer, format)
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|error| error.to_string())?;
    drop(writer); // Close the file before publishing the completion marker.
    Ok(())
}

/// This is an explicit one-shot batch option, never the interactive quit path.
/// On the Windows software-D3D12 runner, AppExit stopped frames after saving the
/// image but teardown did not terminate the process within 180 seconds. Avoid
/// waiting on renderer/window destructors after all requested output is closed.
fn finish_batch_capture(requested: bool, result: &Result<(), String>) {
    if !requested {
        return;
    }
    let status = i32::from(result.is_err());
    eprintln!("Batch capture complete: status {status}");
    let stdout_ok = std::io::stdout().flush().is_ok();
    let stderr_ok = std::io::stderr().flush().is_ok();
    std::process::exit(if stdout_ok && stderr_ok { status } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};
    use flightsim_core::{Degrees, Meters};
    use flightsim_world::{
        DemTile, EmptyTileSource, HeightGrid, LodSelector, MemoryTileSource, Runway, TerrainError,
        TileCache, TileSource,
    };

    fn capture_app() -> App {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(Startup {
                screenshot: Some("unused-proof.png".into()),
                screenshot_delay: 0.0,
                ..default()
            })
            .insert_resource(TerrainTiles::default())
            .insert_resource(CameraWorldPosition(Runway::synthetic().center()))
            .insert_resource(RenderOrigin::new(Geodetic::from_degrees(
                35.55, 139.78, 0.0,
            )))
            .insert_resource(TerrainStreaming {
                selector: LodSelector::new(
                    16.0,
                    720.0,
                    Degrees(60.0).to_radians(),
                    13,
                    Meters(20_000.0),
                ),
                source: Box::new(EmptyTileSource),
                cache: TileCache::new(1024 * 1024),
                live: default(),
                material: default(),
            });
        configure(&mut app);
        app
    }

    fn requests(app: &mut App) -> usize {
        app.world_mut()
            .query::<&Screenshot>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn pending_roots_and_equal_counts_with_different_ids_are_not_ready() {
        let roots = TileId::roots();
        let target: Vec<_> = (0..41).map(|x| TileId::new(6, x, 0)).collect();
        let ready = |matches, stitching, displayed: &[TileId], live: &[TileId]| {
            terrain_ready(
                false,
                matches,
                stitching,
                displayed.iter().copied(),
                live.iter().copied(),
                default(),
            )
        };
        // The captured startup regression: 2 displayed roots, 41 live leaves.
        assert!(!ready(true, true, &roots, &target));
        assert!(!ready(true, false, &roots, &target));
        assert!(!ready(true, false, &roots[..1], &roots[1..]));
        assert!(!ready(false, false, &target, &target));
        assert!(!ready(true, true, &target, &target));
        assert!(ready(true, false, &target, &target));
    }

    #[test]
    fn overlay_transactions_must_commit_but_optional_omission_is_valid() {
        let ready =
            |overlays| terrain_ready(true, true, false, [].into_iter(), [].into_iter(), overlays);
        assert!(ready(default()));
        for overlays in [
            TerrainOverlayUsage {
                pending: true,
                ..default()
            },
            TerrainOverlayUsage {
                dirty: true,
                ..default()
            },
            TerrainOverlayUsage {
                optional_swap_pending: true,
                ..default()
            },
            TerrainOverlayUsage {
                revision: 2,
                committed_revision: 1,
                ..default()
            },
        ] {
            assert!(!ready(overlays));
        }
        assert!(ready(TerrainOverlayUsage {
            revision: 2,
            committed_revision: 2,
            precision_hidden: 1,
            optional_registered: 1,
            omitted_optional: 1,
            ..default()
        }));
    }

    #[test]
    fn no_global_source_preserves_empty_and_sparse_availability_fallbacks() {
        let roots = TileId::roots();
        assert!(terrain_ready(
            true,
            false,
            false,
            [].into_iter(),
            [].into_iter(),
            default()
        ));
        assert!(terrain_ready(
            true,
            false,
            false,
            roots.into_iter(),
            roots.into_iter(),
            default()
        ));
        assert!(!terrain_ready(
            true,
            false,
            true,
            roots.into_iter(),
            roots.into_iter(),
            default()
        ));
        assert!(!terrain_ready(
            true,
            false,
            false,
            roots[..1].iter().copied(),
            roots.into_iter(),
            default()
        ));

        let mut app = capture_app();
        // Exercise the real selector with absent data, rather than assuming its
        // empty state still claims to match the desired globe after an update.
        let camera = app.world().resource::<RenderOrigin>().0.anchor().to_ecef();
        let mut streaming = app.world_mut().resource_mut::<TerrainStreaming>();
        let TerrainStreaming {
            selector,
            source,
            cache,
            live,
            ..
        } = &mut *streaming;
        live.observe_readiness(true);
        let update = flightsim_render::update_terrain_selection(
            selector,
            source,
            cache,
            live,
            camera,
            8,
            &mut |_, _| panic!("empty source built a mesh"),
        );
        assert!(!live.matches_desired());
        assert!(live.is_empty());
        let observed = live.observed_readiness();
        app.world_mut()
            .resource_mut::<CaptureTerrainReadiness>()
            .observe(&update, observed);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(requests(&mut app), 1);
    }

    struct CappedGlobalSource {
        primary: MemoryTileSource,
        primary_possible: bool,
        fallback_max_level: u8,
    }

    impl TileSource for CappedGlobalSource {
        fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
            self.primary.load(id)
        }
        fn primary_reads_possible(&self) -> bool {
            self.primary_possible
        }
        fn has_fallback(&self) -> bool {
            true
        }
        fn load_fallback(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
            Ok((id.level <= self.fallback_max_level)
                .then(|| DemTile::new(id.bounds(), HeightGrid::flat(3, 3, Meters::ZERO))))
        }
    }

    #[test]
    fn real_selector_finishes_valid_coarse_primary_and_capped_global_cuts() {
        for primary_possible in [true, false] {
            let mut source = CappedGlobalSource {
                primary: MemoryTileSource::new(),
                primary_possible,
                fallback_max_level: if primary_possible { 1 } else { 0 },
            };
            if primary_possible {
                for root in TileId::roots() {
                    source.primary.insert(
                        root,
                        DemTile::new(root.bounds(), HeightGrid::flat(3, 3, Meters::ZERO)),
                    );
                }
            }
            let selector = LodSelector::new(1.0, 720.0, Degrees(60.0).to_radians(), 1, Meters(1e9));
            let mut cache = TileCache::new(1024 * 1024);
            let mut live = flightsim_render::TerrainSelectionState::default();
            live.observe_readiness(true);
            let mut readiness = CaptureTerrainReadiness::default();
            for frame in 0..64 {
                let update = flightsim_render::update_terrain_selection(
                    &selector,
                    &source,
                    &mut cache,
                    &mut live,
                    Runway::synthetic().center().to_ecef(),
                    1,
                    &mut |_, _| {},
                );
                readiness.observe(&update, live.observed_readiness());
                if frame == 0 {
                    assert!(!readiness.settled);
                }
                if readiness.settled {
                    break;
                }
            }
            assert!(
                readiness.settled,
                "legitimate availability cut never settled"
            );
            assert!(
                !live.matches_desired(),
                "fixture must preserve a coarser cut"
            );
            assert_eq!(live.ids().collect::<Vec<_>>(), TileId::roots());
            assert_eq!(live.fallback_len(), if primary_possible { 0 } else { 2 });
            assert!(terrain_ready(
                readiness.settled,
                live.matches_desired(),
                false,
                live.ids(),
                live.ids(),
                default()
            ));
        }
    }

    #[test]
    fn readiness_observation_is_absent_without_a_request_and_removed_after_capture() {
        let mut app = capture_app();
        assert!(app.world().contains_resource::<CaptureTerrainReadiness>());
        for _ in 0..30 {
            app.update();
        }
        assert!(!app.world().contains_resource::<CaptureTerrainReadiness>());
        app.update();
        assert_eq!(requests(&mut app), 1);

        let mut interactive = App::new();
        interactive.insert_resource(Startup::default());
        configure(&mut interactive);
        assert!(
            !interactive
                .world()
                .contains_resource::<CaptureTerrainReadiness>()
        );
    }

    #[test]
    fn hidden_or_missing_active_runway_prevents_capture_until_a_full_ready_frame() {
        let mut app = capture_app();
        app.insert_resource(ActiveRunway(Runway::synthetic()));
        for _ in 0..31 {
            app.update();
        }
        assert_eq!(requests(&mut app), 0);
        let runway = app
            .world_mut()
            .spawn((
                RunwaySurface,
                Mesh3d::default(),
                InheritedVisibility::HIDDEN,
            ))
            .id();
        for _ in 0..2 {
            app.update();
        }
        assert_eq!(requests(&mut app), 0);
        // Precision gating hides this same propagated runway component. Last
        // must observe the PostUpdate command, then allow one extraction frame.
        app.add_systems(PostUpdate, move |mut commands: Commands| {
            commands.entity(runway).insert(InheritedVisibility::VISIBLE);
        });
        app.update();
        assert_eq!(requests(&mut app), 0);
        app.update();
        assert_eq!(requests(&mut app), 1);
    }

    #[test]
    fn distant_synthetic_runway_does_not_block_worldwide_free_flight() {
        let mut app = capture_app();
        app.insert_resource(ActiveRunway(Runway::synthetic()));
        let remote = Geodetic::from_degrees(46.58, 8.0, 1500.0);
        app.insert_resource(CameraWorldPosition(remote));
        app.insert_resource(RenderOrigin::new(remote));
        app.world_mut().spawn((
            RunwaySurface,
            Mesh3d::default(),
            InheritedVisibility::HIDDEN,
        ));
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(requests(&mut app), 1);
    }

    #[test]
    fn replacement_or_rebase_invalidates_the_previous_ready_frame() {
        let mut app = capture_app();
        let first = app.world_mut().spawn_empty().id();
        let replacement = app.world_mut().spawn_empty().id();
        let snapshot = || ReadyScene {
            startup_revision: 1,
            origin: Geodetic::from_degrees(35.55, 139.78, 0.0),
            surfaces: vec![(TileId::roots()[0], first)],
            overlay_revision: 1,
            precision_hidden: 0,
            runway: Some((first, Handle::<Mesh>::default().id())),
            models: vec![first],
        };
        let mut state = CaptureState::default();
        assert!(!state.observe(Some(snapshot())));
        assert!(state.observe(Some(snapshot())));
        assert!(!state.observe(None));
        assert!(!state.observe(Some(snapshot())));
        for replacement_snapshot in [
            ReadyScene {
                surfaces: vec![(TileId::roots()[0], replacement)],
                ..snapshot()
            },
            ReadyScene {
                overlay_revision: 2,
                ..snapshot()
            },
            ReadyScene {
                origin: Geodetic::from_degrees(36.0, 139.78, 0.0),
                ..snapshot()
            },
            ReadyScene {
                runway: Some((replacement, Handle::<Mesh>::default().id())),
                ..snapshot()
            },
            ReadyScene {
                models: vec![replacement],
                ..snapshot()
            },
            ReadyScene {
                startup_revision: 2,
                ..snapshot()
            },
        ] {
            state.previous_ready = Some(snapshot());
            assert!(!state.observe(Some(replacement_snapshot)));
        }
    }

    #[test]
    fn model_fit_completion_needs_a_subsequent_ready_frame() {
        let mut app = capture_app();
        let model = app
            .world_mut()
            .spawn((ExteriorModel, PendingModelFit(Startup::default().model_fit)))
            .id();
        for _ in 0..31 {
            app.update();
        }
        assert_eq!(requests(&mut app), 0);
        app.add_systems(Update, move |mut commands: Commands| {
            commands.entity(model).remove::<PendingModelFit>();
        });
        app.update();
        assert_eq!(requests(&mut app), 0);
        app.update();
        assert_eq!(requests(&mut app), 1);
    }

    #[test]
    fn diagnostic_arms_once_at_the_ordinary_thirtieth_frame_request() {
        for enabled in [false, true] {
            let mut app = capture_app();
            app.world_mut()
                .resource_mut::<Startup>()
                .windows_readback_diagnostic = enabled;
            for _ in 0..29 {
                app.update();
            }
            assert_eq!(
                app.world_mut()
                    .query::<&Screenshot>()
                    .iter(app.world())
                    .count(),
                0
            );
            assert!(
                !app.world()
                    .contains_resource::<crate::windows_readback_diagnostic::ProbeRequest>()
            );
            app.update();
            assert_eq!(
                app.world_mut()
                    .query::<&Screenshot>()
                    .iter(app.world())
                    .count(),
                1
            );
            assert_eq!(
                app.world()
                    .contains_resource::<crate::windows_readback_diagnostic::ProbeRequest>(),
                enabled
            );
            app.update();
            assert_eq!(
                app.world_mut()
                    .query::<&Screenshot>()
                    .iter(app.world())
                    .count(),
                1
            );
        }
    }

    fn image() -> Image {
        Image::new_fill(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[10, 20, 30, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }

    #[test]
    fn png_completion_and_write_errors_are_reported() {
        let directory =
            std::env::temp_dir().join(format!("flightsim-capture-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("complete.png");
        save_capture_image(image(), &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            &bytes[bytes.len() - 12..],
            &[0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130]
        );
        assert!(save_capture_image(image(), &directory.join("missing/failed.png")).is_err());
        assert!(save_capture_image(image(), &directory.join("wrong.jpg")).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn batch_admission_preserves_delay_and_30_updates_then_requires_a_prior_opportunity() {
        use crate::capture_admission::CaptureAdmission;
        use std::sync::atomic::Ordering;
        for (delay, eligible_at) in [(0.0, 30), (40.0, 40)] {
            let mut app = capture_app();
            app.insert_resource(CaptureAdmission::default());
            app.world_mut().resource_mut::<Startup>().screenshot_delay = delay;
            for frame in 1..=eligible_at {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_secs(1));
                app.update();
                assert_eq!(
                    requests(&mut app),
                    0,
                    "frame {frame} must not capture immediately"
                );
                assert_eq!(
                    app.world()
                        .resource::<CaptureAdmission>()
                        .test_opportunity()
                        .is_some(),
                    frame == eligible_at
                );
            }
            let (epoch, ack) = app
                .world()
                .resource::<CaptureAdmission>()
                .test_opportunity()
                .unwrap();
            app.update();
            assert_eq!(
                requests(&mut app),
                0,
                "a pending render cannot acknowledge itself"
            );
            ack.store(epoch, Ordering::Release);
            app.update();
            assert_eq!(requests(&mut app), 1);
            assert!(
                app.world()
                    .resource::<CaptureAdmission>()
                    .test_opportunity()
                    .is_none()
            );
            app.update();
            assert_eq!(requests(&mut app), 1);
        }
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn a_changed_ready_scene_rejects_late_render_acknowledgements() {
        use crate::capture_admission::CaptureAdmission;
        use std::sync::atomic::Ordering;
        let mut app = capture_app();
        app.insert_resource(CaptureAdmission::default());
        for _ in 0..30 {
            app.update();
        }
        let (old_epoch, old_ack) = app
            .world()
            .resource::<CaptureAdmission>()
            .test_opportunity()
            .unwrap();
        app.insert_resource(RenderOrigin::new(Geodetic::from_degrees(36.0, 139.78, 0.0)));
        old_ack.store(old_epoch, Ordering::Release);
        app.update();
        assert_eq!(requests(&mut app), 0);
        assert!(
            app.world()
                .resource::<CaptureAdmission>()
                .test_opportunity()
                .is_none()
        );
        app.update();
        let (new_epoch, new_ack) = app
            .world()
            .resource::<CaptureAdmission>()
            .test_opportunity()
            .unwrap();
        assert_ne!(old_epoch, new_epoch);
        old_ack.store(old_epoch, Ordering::Release);
        app.update();
        assert_eq!(requests(&mut app), 0);
        new_ack.store(new_epoch, Ordering::Release);
        app.update();
        assert_eq!(requests(&mut app), 1);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn screenshot_events_end_backpressure_on_save_success_failure_and_cancellation() {
        let directory = std::env::temp_dir().join(format!(
            "flightsim-capture-session-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        for (name, remove_request, saved) in [
            ("complete.png", false, true),
            ("invalid.jpg", false, false),
            ("cancelled.png", true, false),
        ] {
            let mut app = capture_app();
            let session = crate::capture_backpressure::CaptureSession::default();
            app.insert_resource(session.clone());
            app.insert_resource(crate::capture_admission::CaptureAdmission::default());
            let path = directory.join(name);
            app.world_mut().resource_mut::<Startup>().screenshot = Some(path.clone());
            for _ in 0..30 {
                app.update();
            }
            assert_eq!(requests(&mut app), 0);
            let (epoch, ack) = app
                .world()
                .resource::<crate::capture_admission::CaptureAdmission>()
                .test_opportunity()
                .unwrap();
            ack.store(epoch, std::sync::atomic::Ordering::Release);
            app.update();
            assert_eq!(requests(&mut app), 1);
            let entity = app
                .world_mut()
                .query_filtered::<Entity, With<Screenshot>>()
                .single(app.world())
                .unwrap();
            if remove_request {
                app.world_mut().resource_mut::<Startup>().screenshot = None;
            }
            assert!(session.active());
            app.world_mut().trigger(ScreenshotCaptured {
                entity,
                image: image(),
            });
            assert!(!session.active());
            // A duplicate/late delivery cannot reopen the one-shot session.
            app.world_mut().trigger(ScreenshotCaptured {
                entity,
                image: image(),
            });
            assert!(!session.active());
            assert_eq!(
                app.world()
                    .resource::<crate::capture_admission::CaptureAdmission>()
                    .test_acknowledged_epoch(),
                0
            );
            assert_eq!(path.exists(), saved);
            if saved {
                std::fs::remove_file(path).unwrap();
            }
        }
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn batch_exit_child() {
        let Ok(mode) = std::env::var("FLIGHTSIM_CAPTURE_EXIT_TEST") else {
            return;
        };
        print!("stdout proof before exit");
        eprintln!("stderr proof before exit");
        let result = if mode == "failure" {
            Err("injected write failure".into())
        } else {
            Ok(())
        };
        finish_batch_capture(mode != "interactive", &result);
        println!("interactive continued");
    }

    #[test]
    fn batch_status_flush_and_interactive_nontermination_are_regressed() {
        for (mode, code) in [("success", 0), ("failure", 1), ("interactive", 0)] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "screen_capture::tests::batch_exit_child",
                    "--nocapture",
                ])
                .env("FLIGHTSIM_CAPTURE_EXIT_TEST", mode)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(code), "{mode}");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stdout.contains("stdout proof before exit"));
            assert!(stderr.contains("stderr proof before exit"));
            assert_eq!(
                stdout.contains("interactive continued"),
                mode == "interactive"
            );
            assert_eq!(
                stderr.contains("Batch capture complete:"),
                mode != "interactive"
            );
        }
    }
}
