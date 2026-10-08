//! Defer unfinished view draws during native batch screenshot preparation.
//!
//! Every Main/Extract/Render preparation, upload and cleanup still runs. Only
//! the camera driver's view list is temporarily empty during loading; Bevy's
//! normal no-camera window clear remains. The 30-update floor counts real Main
//! updates, not full-scene draws. One same-scene admitted Render opportunity is
//! required afterward, without claiming GPU/shader completion (ADR-0030).

use bevy::camera::NormalizedRenderTarget;
use bevy::ecs::entity::ContainsEntity;
use bevy::ecs::schedule::{InternedSystemSet, IntoSystemSet, SystemSet};
use bevy::prelude::*;
use bevy::render::camera::{ExtractedCamera, SortedCamera, SortedCameras};
use bevy::render::renderer::render_system;
use bevy::render::sync_world::RenderEntity;
use bevy::render::view::screenshot::Screenshot;
use bevy::render::view::{ExtractedWindows, ViewTarget};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "capture_admission_state.rs"]
mod state;
use state::{AdmissionState, Snapshot};

#[derive(Resource, Default)]
pub(super) struct CaptureAdmission {
    state: AdmissionState,
    rendered_epoch: Arc<AtomicU64>,
}

impl CaptureAdmission {
    pub(super) fn observe(&mut self, scene_changed: bool, eligible: bool) -> bool {
        let was_open = self.state.snapshot().draw_views;
        let allowed = self
            .state
            .observe(
                scene_changed,
                eligible,
                self.rendered_epoch.load(Ordering::Acquire),
            )
            .unwrap_or_else(|error| {
                self.rendered_epoch.store(0, Ordering::Release);
                panic!("{error}")
            });
        if !was_open && self.state.snapshot().draw_views {
            info!(
                "screenshot preparation eligible: admitting full-scene views before requesting capture; not GPU completion"
            );
        }
        allowed
    }

    pub(super) fn requested(&mut self) {
        self.state.requested();
    }

    pub(super) fn finish(&mut self) {
        self.state.finish();
        self.rendered_epoch.store(0, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn test_opportunity(&self) -> Option<(u64, Arc<AtomicU64>)> {
        self.state
            .snapshot()
            .opportunity_epoch
            .map(|epoch| (epoch, self.rendered_epoch.clone()))
    }

    #[cfg(test)]
    pub(super) fn test_acknowledged_epoch(&self) -> u64 {
        self.rendered_epoch.load(Ordering::Acquire)
    }
}

#[derive(Resource)]
struct RenderAdmission {
    snapshot: Snapshot,
    rendered_epoch: Arc<AtomicU64>,
    flight_camera: Option<Entity>,
    screenshot_in_flight: bool,
}

#[derive(Resource, Default)]
struct FrameViews {
    // Some(empty) must still be restored, independently of later session state.
    saved: Option<Vec<SortedCamera>>,
    opportunity: Option<(u64, Arc<AtomicU64>)>,
}

pub(super) fn configure(app: &mut App) {
    let startup = app.world().resource::<crate::Startup>();
    if startup.screenshot.is_none() || !startup.exit_after_screenshot {
        return;
    }
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<FrameViews>()
        .add_systems(ExtractSchedule, extract_admission);
    configure_frame_hooks(render_app, render_system.into_system_set().intern());
    app.init_resource::<CaptureAdmission>()
        .add_systems(Last, cancel_removed_request);
}

fn configure_frame_hooks(render_app: &mut bevy::app::SubApp, renderer: InternedSystemSet) {
    // Both hooks are after all preparation. Bevy's original pipeline-cache
    // processing still precedes render_system and does not consume this list.
    // Use a renderer boundary seam so lifecycle tests can execute the actual
    // hooks with a GPU-free render body; production keeps Bevy's registration.
    render_app.add_systems(
        Render,
        (
            prepare_view_admission.before(renderer),
            restore_views.after(renderer),
        )
            .in_set(RenderSystems::Render),
    );
}

fn cancel_removed_request(startup: Res<crate::Startup>, mut admission: ResMut<CaptureAdmission>) {
    if startup.screenshot.is_none() {
        admission.finish();
    }
}

fn extract_admission(
    admission: Extract<Res<CaptureAdmission>>,
    flight_camera: Extract<Query<RenderEntity, With<crate::world_runtime::FlightCamera>>>,
    screenshots: Extract<Query<(), With<Screenshot>>>,
    mut commands: Commands,
) {
    commands.insert_resource(RenderAdmission {
        snapshot: admission.state.snapshot(),
        rendered_epoch: admission.rendered_epoch.clone(),
        flight_camera: flight_camera.single().ok(),
        // Defensive: never let admission suppress an extracted screenshot,
        // including a late delivery after cancellation or a separate request.
        screenshot_in_flight: !screenshots.is_empty(),
    });
}

fn prepare_view_admission(
    admission: Res<RenderAdmission>,
    mut cameras: ResMut<SortedCameras>,
    mut frame: ResMut<FrameViews>,
    views: Query<(&ExtractedCamera, &ViewTarget), With<Camera3d>>,
    windows: Res<ExtractedWindows>,
) {
    assert!(frame.saved.is_none(), "capture view list was not restored");
    frame.opportunity = None;
    if !admission.snapshot.draw_views && !admission.screenshot_in_flight {
        frame.saved = Some(std::mem::take(&mut cameras.0));
        return;
    }
    let Some(epoch) = admission.snapshot.opportunity_epoch else {
        return;
    };
    let Some(entity) = admission.flight_camera else {
        return;
    };
    if !cameras.0.iter().any(|camera| camera.entity == entity) {
        return;
    }
    let Ok((camera, _target)) = views.get(entity) else {
        return;
    };
    if camera
        .physical_viewport_size
        .is_none_or(|size| size.min_element() == 0)
        || camera
            .physical_target_size
            .is_none_or(|size| size.min_element() == 0)
    {
        return;
    }
    let target_valid = match &camera.target {
        Some(NormalizedRenderTarget::Window(window)) => windows
            .get(&window.entity())
            .is_some_and(|window| window.physical_width > 0 && window.physical_height > 0),
        Some(NormalizedRenderTarget::Image(_) | NormalizedRenderTarget::TextureView(_)) => true,
        _ => false,
    };
    if target_valid {
        frame.opportunity = Some((epoch, admission.rendered_epoch.clone()));
    }
}

fn restore_views(mut cameras: ResMut<SortedCameras>, mut frame: ResMut<FrameViews>) {
    if let Some(saved) = frame.saved.take() {
        cameras.0 = saved;
    }
}

/// Preserve the saved view list even if a render system unwinds. The failure
/// is resumed immediately; this is cleanup, never renderer recovery or success.
/// Non-batch screenshot sessions have no FrameViews and keep the direct path.
pub(super) fn with_restored_views(world: &mut World, render: impl FnOnce(&mut World)) {
    if !world.contains_resource::<FrameViews>() {
        render(world);
        return;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(world)));
    if let Err(failure) = result {
        let saved = if let Some(mut frame) = world.get_resource_mut::<FrameViews>() {
            frame.opportunity = None;
            frame.saved.take()
        } else {
            None
        };
        if let Some(saved) = saved {
            world.resource_mut::<SortedCameras>().0 = saved;
        }
        if let Some(admission) = world.get_resource::<RenderAdmission>() {
            admission.rendered_epoch.store(0, Ordering::Release);
        }
        std::panic::resume_unwind(failure);
    }
}

/// Called only after the *entire* original Render schedule returns. A failed
/// renderer cannot acknowledge an opportunity. The existing queue credit
/// callback remains the separate authority for GPU frame completion.
pub(super) fn acknowledge_render(world: &mut World) {
    let Some(mut frame) = world.get_resource_mut::<FrameViews>() else {
        return;
    };
    assert!(frame.saved.is_none(), "capture view list was not restored");
    if let Some((epoch, rendered)) = frame.opportunity.take() {
        rendered.store(epoch, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::schedule::ScheduleLabel;

    #[derive(Resource, Default)]
    struct Lifecycle(Vec<(&'static str, usize)>);

    fn prepare(cameras: Res<SortedCameras>, mut log: ResMut<Lifecycle>) {
        log.0.push(("prepare", cameras.0.len()));
    }
    #[derive(Resource)]
    struct FailRender;

    fn render(
        cameras: Res<SortedCameras>,
        mut log: ResMut<Lifecycle>,
        failure: Option<Res<FailRender>>,
    ) {
        log.0.push(("render", cameras.0.len()));
        assert!(failure.is_none(), "injected render failure");
    }
    fn cleanup(cameras: Res<SortedCameras>, mut log: ResMut<Lifecycle>) {
        log.0.push(("cleanup", cameras.0.len()));
    }

    fn render_app(snapshot: Snapshot, screenshot_in_flight: bool) -> bevy::app::SubApp {
        let mut app = bevy::app::SubApp::new();
        app.add_schedule(Render::base_schedule());
        app.update_schedule = Some(Render.intern());
        let first = app.world_mut().spawn_empty().id();
        let second = app.world_mut().spawn_empty().id();
        app.insert_resource(RenderAdmission {
            snapshot,
            rendered_epoch: Arc::default(),
            flight_camera: Some(first),
            screenshot_in_flight,
        })
        .insert_resource(SortedCameras(vec![
            SortedCamera {
                entity: first,
                order: -3,
                target: None,
                hdr: true,
            },
            SortedCamera {
                entity: second,
                order: 7,
                target: None,
                hdr: false,
            },
        ]))
        .init_resource::<FrameViews>()
        .init_resource::<ExtractedWindows>()
        .init_resource::<Lifecycle>()
        .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
        .add_systems(Render, render.in_set(RenderSystems::Render))
        .add_systems(Render, cleanup.in_set(RenderSystems::Cleanup));
        configure_frame_hooks(&mut app, render.into_system_set().intern());
        app
    }

    #[test]
    fn loading_keeps_preparation_and_cleanup_and_restores_the_exact_view_list() {
        let mut app = render_app(AdmissionState::default().snapshot(), false);
        let original: Vec<_> = app
            .world()
            .resource::<SortedCameras>()
            .0
            .iter()
            .map(|c| (c.entity, c.order, c.target.clone(), c.hdr))
            .collect();
        for _ in 0..3 {
            app.update();
            acknowledge_render(app.world_mut());
            assert_eq!(
                app.world()
                    .resource::<RenderAdmission>()
                    .rendered_epoch
                    .load(Ordering::Acquire),
                0
            );
            let restored: Vec<_> = app
                .world()
                .resource::<SortedCameras>()
                .0
                .iter()
                .map(|c| (c.entity, c.order, c.target.clone(), c.hdr))
                .collect();
            assert_eq!(restored, original);
            assert!(app.world().resource::<FrameViews>().saved.is_none());
        }
        assert_eq!(
            app.world().resource::<Lifecycle>().0,
            [("prepare", 2), ("render", 0), ("cleanup", 2)].repeat(3)
        );
    }

    #[test]
    fn an_extracted_screenshot_never_loses_its_view_draws() {
        let mut app = render_app(AdmissionState::default().snapshot(), true);
        app.update();
        acknowledge_render(app.world_mut());
        assert_eq!(
            app.world().resource::<Lifecycle>().0,
            [("prepare", 2), ("render", 2), ("cleanup", 2)]
        );
        assert!(app.world().resource::<FrameViews>().saved.is_none());
    }

    #[test]
    fn restoration_does_not_depend_on_a_later_snapshot_or_empty_saved_list() {
        let mut app = render_app(AdmissionState::default().snapshot(), false);
        let saved = std::mem::take(&mut app.world_mut().resource_mut::<SortedCameras>().0);
        app.world_mut().resource_mut::<FrameViews>().saved = Some(saved);
        app.world_mut()
            .resource_mut::<RenderAdmission>()
            .snapshot
            .draw_views = true;
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut().run_system_once(restore_views).unwrap();
        assert_eq!(app.world().resource::<SortedCameras>().0.len(), 2);
        app.world_mut().resource_mut::<FrameViews>().saved = Some(Vec::new());
        app.world_mut().run_system_once(restore_views).unwrap();
        assert!(app.world().resource::<SortedCameras>().0.is_empty());
        assert!(app.world().resource::<FrameViews>().saved.is_none());
    }

    #[test]
    fn admission_with_no_valid_flight_view_does_not_acknowledge_a_render() {
        let mut state = AdmissionState::default();
        assert_eq!(state.observe(true, true, 0), Ok(false));
        let mut app = render_app(state.snapshot(), false);
        app.update();
        acknowledge_render(app.world_mut());
        let ack = app
            .world()
            .resource::<RenderAdmission>()
            .rendered_epoch
            .load(Ordering::Acquire);
        assert_eq!(ack, 0);
        assert_eq!(state.observe(false, true, ack), Ok(false));
        assert_eq!(
            app.world().resource::<Lifecycle>().0,
            [("prepare", 2), ("render", 2), ("cleanup", 2)]
        );
    }

    #[test]
    fn acknowledgement_uses_the_admitted_frame_token_not_later_main_state() {
        let mut app = render_app(AdmissionState::default().snapshot(), false);
        let prior_ack = Arc::new(AtomicU64::new(0));
        app.world_mut().resource_mut::<FrameViews>().opportunity = Some((4, prior_ack.clone()));
        let next_ack = Arc::new(AtomicU64::new(0));
        app.world_mut()
            .resource_mut::<RenderAdmission>()
            .rendered_epoch = next_ack.clone();
        acknowledge_render(app.world_mut());
        acknowledge_render(app.world_mut());
        assert_eq!(prior_ack.load(Ordering::Acquire), 4);
        assert_eq!(next_ack.load(Ordering::Acquire), 0);
        let mut state = AdmissionState::default();
        for _ in 0..5 {
            assert_eq!(state.observe(true, true, 0), Ok(false));
        }
        assert_eq!(
            state.observe(false, true, prior_ack.load(Ordering::Acquire)),
            Ok(false)
        );
    }

    #[test]
    fn a_failed_render_restores_views_invalidates_ack_and_still_fails() {
        let mut app = render_app(AdmissionState::default().snapshot(), false);
        let original: Vec<_> = app
            .world()
            .resource::<SortedCameras>()
            .0
            .iter()
            .map(|camera| camera.entity)
            .collect();
        app.insert_resource(FailRender);
        let ack = app
            .world()
            .resource::<RenderAdmission>()
            .rendered_epoch
            .clone();
        ack.store(7, Ordering::Release);
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_restored_views(app.world_mut(), |world| world.run_schedule(Render));
            acknowledge_render(app.world_mut());
        }));
        assert!(failed.is_err());
        assert_eq!(ack.load(Ordering::Acquire), 0);
        assert!(app.world().resource::<FrameViews>().saved.is_none());
        assert!(app.world().resource::<FrameViews>().opportunity.is_none());
        assert_eq!(
            app.world()
                .resource::<SortedCameras>()
                .0
                .iter()
                .map(|camera| camera.entity)
                .collect::<Vec<_>>(),
            original
        );
    }

    #[test]
    fn batch_configuration_extracts_a_deferred_snapshot_and_render_entity_mapping() {
        use bevy::render::MainWorld;
        let mut app = App::new();
        app.insert_resource(crate::Startup {
            screenshot: Some("test.png".into()),
            exit_after_screenshot: true,
            ..default()
        });
        let mut render_app = bevy::app::SubApp::new();
        let mut extraction = Schedule::new(ExtractSchedule);
        extraction.set_apply_final_deferred(false);
        render_app.add_schedule(extraction);
        let render_entity = render_app.world_mut().spawn_empty().id();
        app.insert_sub_app(RenderApp, render_app);
        configure(&mut app);
        assert!(app.world().contains_resource::<CaptureAdmission>());
        for _ in 0..5 {
            app.world_mut().spawn_empty();
        }
        let main_entity = app
            .world_mut()
            .spawn((
                crate::world_runtime::FlightCamera,
                RenderEntity::from(render_entity),
            ))
            .id();
        assert_ne!(main_entity, render_entity);
        assert!(
            !app.world_mut()
                .resource_mut::<CaptureAdmission>()
                .observe(true, true)
        );
        let mut render_app = app.remove_sub_app(RenderApp).unwrap();

        // Use the real ExtractSchedule registration and its deferred commands.
        // MainWorld exposes a World through DerefMut, so this requires no GPU
        // initialization or private Bevy constructor.
        let extract = |app: &mut App, render_app: &mut bevy::app::SubApp| {
            let mut main = MainWorld::default();
            std::mem::swap(&mut *main, app.world_mut());
            render_app.world_mut().insert_resource(main);
            render_app.world_mut().run_schedule(ExtractSchedule);
            let mut main = render_app
                .world_mut()
                .remove_resource::<MainWorld>()
                .unwrap();
            std::mem::swap(&mut *main, app.world_mut());
        };
        extract(&mut app, &mut render_app);
        assert!(!render_app.world().contains_resource::<RenderAdmission>());
        assert!(
            !app.world_mut()
                .resource_mut::<CaptureAdmission>()
                .observe(true, false)
        );
        app.world_mut().spawn(Screenshot::primary_window());
        render_app
            .world_mut()
            .schedule_scope(ExtractSchedule, |world, schedule| {
                schedule.apply_deferred(world);
            });
        let frame = render_app.world().resource::<RenderAdmission>();
        assert_eq!(frame.snapshot.opportunity_epoch, Some(1));
        assert_eq!(frame.flight_camera, Some(render_entity));
        assert!(!frame.screenshot_in_flight);

        extract(&mut app, &mut render_app);
        render_app
            .world_mut()
            .schedule_scope(ExtractSchedule, |world, schedule| {
                schedule.apply_deferred(world);
            });
        let frame = render_app.world().resource::<RenderAdmission>();
        assert_eq!(frame.snapshot.opportunity_epoch, None);
        assert!(frame.screenshot_in_flight);
        app.world_mut().resource_mut::<crate::Startup>().screenshot = None;
        app.update();
        assert!(
            !app.world_mut()
                .resource_mut::<CaptureAdmission>()
                .observe(false, true)
        );
        assert!(
            app.world()
                .resource::<CaptureAdmission>()
                .state
                .snapshot()
                .draw_views
        );
    }

    #[test]
    fn ordinary_and_non_batch_launches_install_no_admission_systems() {
        for (screenshot, batch) in [(false, false), (false, true), (true, false)] {
            let mut app = App::new();
            app.insert_resource(crate::Startup {
                screenshot: screenshot.then(|| "test.png".into()),
                exit_after_screenshot: batch,
                ..default()
            });
            app.insert_sub_app(RenderApp, bevy::app::SubApp::new());
            configure(&mut app);
            assert!(!app.world().contains_resource::<CaptureAdmission>());
            assert!(
                !app.sub_app(RenderApp)
                    .world()
                    .contains_resource::<FrameViews>()
            );
            assert!(app.sub_app(RenderApp).get_schedule(Render).is_none());
            assert!(
                app.sub_app(RenderApp)
                    .get_schedule(ExtractSchedule)
                    .is_none()
            );
        }
    }
}
