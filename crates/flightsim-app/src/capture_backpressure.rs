//! Bound queued render work during an explicit native screenshot session.
//!
//! Queue submission already services callbacks in wgpu. When both frame credits
//! are occupied, this gate deliberately withholds another complete render
//! schedule and services completion instead. It never skips preparation or the
//! screenshot extraction/collection lifecycle, and never claims GPU readiness.

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::render_resource::PollType;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::{Render, RenderApp};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wgpu_types::PollError;

// Bound render-frame batches, not the number of internal wgpu submissions.
const MAX_IN_FLIGHT_FRAMES: usize = 2;
// A servicing wait has a finite GPU wait; the independent process watchdog is
// still the end-to-end deadline, including callbacks and native driver calls.
const SERVICE_WAIT: Duration = Duration::from_millis(100);

#[derive(Resource, Clone)]
pub(super) struct CaptureSession(Arc<AtomicBool>);

impl Default for CaptureSession {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}

impl CaptureSession {
    pub(super) fn finish(&self) {
        self.0.store(false, Ordering::Release);
    }

    pub(super) fn active(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Resource, Default)]
struct FrameCredits {
    pending: VecDeque<Arc<AtomicBool>>,
}

impl FrameCredits {
    fn reclaim(&mut self) {
        while self
            .pending
            .front()
            .is_some_and(|complete| complete.load(Ordering::Acquire))
        {
            self.pending.pop_front();
        }
    }

    /// A timeout is a pending credit, never permission to submit another frame.
    /// Cancellation releases only CPU bookkeeping; late callbacks own their
    /// original atomics and cannot grant credit to another capture session.
    fn wait_for_credit<E>(
        &mut self,
        session: &CaptureSession,
        mut service: impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        loop {
            if !session.active() {
                self.pending.clear();
                return Ok(());
            }
            self.reclaim();
            if self.pending.len() < MAX_IN_FLIGHT_FRAMES {
                return Ok(());
            }
            service()?;
        }
    }

    fn submitted(&mut self) -> Arc<AtomicBool> {
        assert!(self.pending.len() < MAX_IN_FLIGHT_FRAMES);
        let complete = Arc::new(AtomicBool::new(false));
        self.pending.push_back(complete.clone());
        complete
    }
}

#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct CaptureRender;

pub(super) fn configure(app: &mut App) {
    if app
        .world()
        .resource::<crate::Startup>()
        .screenshot
        .is_none()
    {
        return;
    }
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    let session = CaptureSession::default();
    render_app
        .insert_resource(session.clone())
        .init_resource::<FrameCredits>()
        .add_systems(CaptureRender, render_capture_frame);
    // PipelinedRenderPlugin moves this same sub-app to its render thread. Keep
    // its extraction and the complete original Render schedule unchanged.
    render_app.update_schedule = Some(CaptureRender.intern());
    app.insert_resource(session)
        .add_systems(Last, cancel_removed_request);
}

fn cancel_removed_request(startup: Res<crate::Startup>, session: Res<CaptureSession>) {
    if startup.screenshot.is_none() {
        session.finish();
    }
}

fn render_capture_frame(world: &mut World) {
    world.resource_scope(|world, mut credits: Mut<FrameCredits>| {
        let session = world.resource::<CaptureSession>().clone();
        let device = world.resource::<RenderDevice>().clone();
        credits
            .wait_for_credit(&session, || {
                match device.poll(PollType::Wait {
                    // Bevy owns the submission indices; do not fabricate one or
                    // add an empty submission just to obtain an index.
                    submission_index: None,
                    timeout: Some(SERVICE_WAIT),
                }) {
                    Ok(_) | Err(PollError::Timeout) => Ok(()),
                    Err(error) => Err(error),
                }
            })
            .unwrap_or_else(|error| panic!("screenshot GPU backpressure failed: {error}"));

        crate::capture_admission::with_restored_views(world, |world| {
            world.run_schedule(Render);
        });
        crate::capture_admission::acknowledge_render(world);

        if session.active() {
            let complete = credits.submitted();
            // Registered after ALL Render systems, including graph submission,
            // screenshot map-task dispatch and cleanup. The callback does no
            // ECS work and cannot hold up wgpu's callback-service thread.
            world
                .resource::<RenderQueue>()
                .on_submitted_work_done(move || complete.store(true, Ordering::Release));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(ticket: &AtomicBool) {
        ticket.store(true, Ordering::Release);
    }

    #[test]
    fn a_stalled_gpu_cannot_accumulate_more_than_two_frames() {
        let session = CaptureSession::default();
        let mut credits = FrameCredits::default();
        let first = credits.submitted();
        let second = credits.submitted();
        let mut services = 0;
        credits
            .wait_for_credit(&session, || {
                services += 1;
                // Reproduce many successful maintenance calls without fence
                // completion. Only the actual completion returns a credit.
                if services == 136 {
                    complete(&first);
                }
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(services, 136);
        assert_eq!(credits.pending.len(), 1);
        assert!(Arc::ptr_eq(credits.pending.front().unwrap(), &second));
        let _third = credits.submitted();
        assert_eq!(credits.pending.len(), MAX_IN_FLIGHT_FRAMES);
    }

    #[test]
    fn completed_batches_release_credit_without_extra_service() {
        let session = CaptureSession::default();
        let mut credits = FrameCredits::default();
        for _ in 0..100 {
            credits
                .wait_for_credit(&session, || -> Result<(), ()> {
                    panic!("a free credit must not poll")
                })
                .unwrap();
            complete(&credits.submitted());
        }
        credits.reclaim();
        assert!(credits.pending.is_empty());
    }

    #[test]
    fn out_of_order_and_repeated_callbacks_do_not_mint_credits() {
        let session = CaptureSession::default();
        let mut credits = FrameCredits::default();
        let first = credits.submitted();
        let second = credits.submitted();
        complete(&second);
        complete(&second);
        let mut services = 0;
        credits
            .wait_for_credit(&session, || {
                services += 1;
                assert_eq!(services, 1);
                complete(&first);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(services, 1);
        assert!(credits.pending.is_empty());
    }

    #[test]
    fn service_errors_preserve_full_credit_debt() {
        let session = CaptureSession::default();
        let mut credits = FrameCredits::default();
        let _first = credits.submitted();
        let _second = credits.submitted();
        assert_eq!(
            credits.wait_for_credit(&session, || Err("device error")),
            Err("device error")
        );
        assert_eq!(credits.pending.len(), MAX_IN_FLIGHT_FRAMES);
    }

    #[test]
    fn cancellation_and_late_callbacks_cannot_complete_a_new_session() {
        let session = CaptureSession::default();
        let mut credits = FrameCredits::default();
        let first = credits.submitted();
        let second = credits.submitted();
        credits
            .wait_for_credit(&session, || {
                session.finish();
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(credits.pending.is_empty());
        session.finish();
        assert!(!session.active());
        let next_session = CaptureSession::default();
        let next = credits.submitted();
        complete(&first);
        complete(&second);
        credits.reclaim();
        assert!(next_session.active());
        assert!(!next.load(Ordering::Acquire));
        assert_eq!(credits.pending.len(), 1);
    }

    #[test]
    fn normal_launch_keeps_its_original_render_schedule() {
        let mut app = App::new();
        app.insert_resource(crate::Startup::default());
        let mut render_app = bevy::app::SubApp::new();
        render_app.update_schedule = Some(Render.intern());
        app.insert_sub_app(RenderApp, render_app);
        configure(&mut app);
        assert!(!app.world().contains_resource::<CaptureSession>());
        let render_app = app.sub_app(RenderApp);
        assert_eq!(render_app.update_schedule, Some(Render.intern()));
        assert!(!render_app.world().contains_resource::<FrameCredits>());
        assert!(render_app.get_schedule(CaptureRender).is_none());
    }

    #[test]
    fn capture_registration_retains_render_and_extraction_then_cancels() {
        #[derive(Resource, Default)]
        struct Extracted(u32);
        let mut app = App::new();
        app.insert_resource(crate::Startup {
            screenshot: Some("test.png".into()),
            ..default()
        });
        let mut render_app = bevy::app::SubApp::new();
        render_app
            .init_resource::<Extracted>()
            .init_schedule(Render);
        render_app.set_extract(|_, render| render.resource_mut::<Extracted>().0 += 1);
        app.insert_sub_app(RenderApp, render_app);
        configure(&mut app);
        let session = app.world().resource::<CaptureSession>().clone();
        assert!(session.active());
        let mut render_app = app.remove_sub_app(RenderApp).unwrap();
        assert_eq!(render_app.update_schedule, Some(CaptureRender.intern()));
        assert!(render_app.get_schedule(Render).is_some());
        render_app.extract(app.world_mut());
        render_app.extract(app.world_mut());
        assert_eq!(render_app.world().resource::<Extracted>().0, 2);
        app.world_mut().resource_mut::<crate::Startup>().screenshot = None;
        app.update();
        assert!(!session.active());
    }
}
