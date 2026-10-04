//! Disabled-by-default, one-shot evidence for a stalled Windows screenshot.
//!
//! This does not access Bevy's private screenshot state. A tiny successful map
//! cannot qualify the real scene capture. The explicit poll may execute arbitrary
//! existing callbacks: its GPU timeout is NOT a process/callback deadline. The
//! candidate's independent 180-second process watchdog remains mandatory.

use bevy::prelude::*;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, LoadOp, MapMode,
    Operations, Origin3d, PollType, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
    TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
};
use bevy::render::renderer::{RenderDevice, RenderQueue, render_system};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::tasks::{AsyncComputeTaskPool, Task};
use std::future::poll_fn;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

const NORMAL_OBSERVATION: Duration = Duration::from_secs(5);
const OBSERVATION_DEADLINE: Duration = Duration::from_secs(15);
const GPU_WAIT_TIMEOUT: Duration = Duration::from_millis(250);
const BUFFER_BYTES: u32 = 1024;
const ROW_PITCH: u32 = 256;

#[derive(Resource, Debug)]
pub(super) struct ProbeRequest;

/// Install no extraction, tasks, buffers or render systems on the ordinary path.
pub(super) fn configure(app: &mut App) {
    if !app
        .world()
        .resource::<crate::Startup>()
        .windows_readback_diagnostic
    {
        return;
    }
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<ProbeState>()
        .add_systems(ExtractSchedule, extract_request)
        .add_systems(
            Render,
            observe_probe
                .after(render_system)
                .in_set(RenderSystems::Render),
        );
    eprintln!("FS_READBACK_PROBE event=enabled");
}

#[derive(Resource, Default)]
struct ProbeState {
    requested: bool,
    finished: bool,
    active: Option<ActiveProbe>,
}

fn extract_request(request: Extract<Option<Res<ProbeRequest>>>, mut state: ResMut<ProbeState>) {
    state.requested |= request.is_some();
}

#[derive(Default)]
struct Signals {
    closed: bool,
    map_result: Option<bool>,
    queue_callback: bool,
    phase: Phase,
    async_started: bool,
    async_waiting: bool,
    async_signal: bool,
    async_resumed: bool,
    waker: Option<Waker>,
}

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    Normal,
    Poll,
    AfterPoll,
    Cleanup,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Poll => "poll",
            Self::AfterPoll => "after_poll",
            Self::Cleanup => "cleanup",
        }
    }
}

fn lock_signals(signals: &Mutex<Signals>) -> MutexGuard<'_, Signals> {
    signals
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct ActiveProbe {
    started: Instant,
    render_frames: u64,
    buffer: Buffer,
    texture: Texture,
    // Contains the actual index returned by our queue.submit; never waits for an
    // unrelated/latest submission and is consumed at most once.
    wait: Option<PollType>,
    poll_status: &'static str,
    pixels: Option<bool>,
    mapping: MappingLifecycle,
    shared: Arc<Mutex<Signals>>,
    task: Option<Task<()>>,
}

/// Callback evidence and ownership are different: a successful callback stays
/// in the summary after check_pixels has released the mapping. Pending evidence
/// is also not a reliable backend state: wgpu can already be Idle after a map
/// error while delivery of its callback is still pending on another thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MappingLifecycle {
    #[default]
    AwaitingCallback,
    Mapped,
    Unmapped,
    MapFailed,
    Destroyed,
}

impl MappingLifecycle {
    fn observe_callback(&mut self, result: Option<bool>) {
        if *self == Self::AwaitingCallback {
            match result {
                Some(true) => *self = Self::Mapped,
                Some(false) => *self = Self::MapFailed,
                None => {}
            }
        }
    }
}

/// wgpu-core 27 Global::buffer_destroy performs internal unmap/cancellation and
/// intentionally tolerates Idle/NotMapped, then destroys. Public unmap instead
/// raises a validation error on Idle. Always use public destroy here: this also
/// handles Pending -> failed/mapped before callback delivery without guessing
/// backend state from callback evidence. No mapped view survives into cleanup.
fn release_mapping(mapping: &mut MappingLifecycle, destroy: impl FnOnce()) {
    if std::mem::replace(mapping, MappingLifecycle::Destroyed) == MappingLifecycle::Destroyed {
        return;
    }
    destroy();
}

fn observe_probe(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut state: ResMut<ProbeState>,
) {
    if !state.requested || state.finished {
        return;
    }
    let Some(probe) = state.active.as_mut() else {
        state.active = Some(start_probe(&device, &queue));
        return;
    };
    probe.render_frames = probe.render_frames.saturating_add(1);
    let elapsed = probe.started.elapsed();
    if elapsed >= OBSERVATION_DEADLINE {
        finish_probe(probe);
        state.active = None;
        state.finished = true;
        return;
    }
    // This later render-frame signal tests an actual externally delivered wake.
    // Merely starting/spawning the task is deliberately separate evidence.
    if let Some(waker) = signal_external(&probe.shared) {
        waker.wake(); // Never wake while the mutex is held.
    }
    check_pixels(probe);
    if elapsed >= NORMAL_OBSERVATION
        && let Some(wait) = probe.wait.take()
    {
        {
            let mut signals = lock_signals(&probe.shared);
            signals.phase = Phase::Poll;
            eprintln!(
                "FS_READBACK_PROBE event=poll_enter gpu_timeout_ms=250 elapsed_ms={}",
                elapsed.as_millis()
            );
        }
        let poll_started = Instant::now();
        // Public Bevy 0.18.1 forwards exactly to wgpu 27 Device::poll. A missing
        // return may reflect batch exit/interruption or a stalled poll/callback;
        // interpret it with the process outcome, never as a claimed 250 ms bound.
        let result = device.poll(wait);
        probe.poll_status = match &result {
            Ok(status) if status.wait_finished() => {
                if status.is_queue_empty() {
                    "queue_empty"
                } else {
                    "wait_succeeded"
                }
            }
            Ok(_) => "unexpected_poll",
            // PollError is not re-exported by Bevy. These are the only two
            // variants in pinned wgpu 27.0.1; retain its Debug value in a normal
            // tracing log, with a bounded machine-readable classification here.
            Err(error) => {
                warn!("readback diagnostic poll error: {error:?}");
                if format!("{error:?}") == "Timeout" {
                    "timeout"
                } else {
                    "wrong_submission"
                }
            }
        };
        {
            let mut signals = lock_signals(&probe.shared);
            eprintln!(
                "FS_READBACK_PROBE event=poll_return status={} wall_ms={}",
                probe.poll_status,
                poll_started.elapsed().as_millis()
            );
            signals.phase = Phase::AfterPoll;
        }
        // The wait may have delivered map callbacks. Record pixels after it,
        // without allowing this to stand in for Bevy's independent screenshot.
        check_pixels(probe);
        if probe.started.elapsed() >= OBSERVATION_DEADLINE {
            finish_probe(probe);
            state.active = None;
            state.finished = true;
        }
    }
}

fn start_probe(device: &RenderDevice, queue: &RenderQueue) -> ActiveProbe {
    let started = Instant::now();
    let size = Extent3d {
        width: 4,
        height: 4,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("flightsim readback diagnostic 4x4"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("flightsim readback diagnostic 1024 bytes"),
        size: u64::from(BUFFER_BYTES),
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("flightsim readback diagnostic clear and copy"),
    });
    {
        let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("flightsim readback diagnostic green clear"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(LinearRgba::new(0.0, 1.0, 0.0, 1.0).into()),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ROW_PITCH),
                rows_per_image: Some(4),
            },
        },
        size,
    );
    let submission = queue.submit([encoder.finish()]);
    eprintln!("FS_READBACK_PROBE event=submitted");
    let shared = Arc::new(Mutex::new(Signals::default()));
    let map_shared = Arc::downgrade(&shared);
    eprintln!("FS_READBACK_PROBE event=map_register_enter");
    // Registered directly on the render thread, outside AsyncComputeTaskPool.
    // Callback holds only a Weak signal reference, never the owned GPU resources.
    buffer.slice(..).map_async(MapMode::Read, move |result| {
        record_map_callback(&map_shared, result.is_ok());
    });
    eprintln!("FS_READBACK_PROBE event=map_registered");
    let queue_shared = Arc::downgrade(&shared);
    queue.on_submitted_work_done(move || record_queue_callback(&queue_shared));
    let task_shared = Arc::clone(&shared);
    let task = AsyncComputeTaskPool::get().spawn(async move {
        wait_for_external_signal(&task_shared).await;
    });
    ActiveProbe {
        started,
        render_frames: 0,
        buffer,
        texture,
        wait: Some(PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(GPU_WAIT_TIMEOUT),
        }),
        poll_status: "not_entered",
        pixels: None,
        mapping: MappingLifecycle::AwaitingCallback,
        shared,
        task: Some(task),
    }
}

fn record_map_callback(shared: &Weak<Mutex<Signals>>, okay: bool) {
    let result = if okay { "ok" } else { "error" };
    if let Some(shared) = shared.upgrade() {
        let mut signals = lock_signals(&shared);
        eprintln!(
            "FS_READBACK_PROBE event=map_callback result={result} cancelled={} phase={}",
            signals.closed,
            signals.phase.name()
        );
        if !signals.closed {
            signals.map_result = Some(okay);
        }
    } else {
        eprintln!(
            "FS_READBACK_PROBE event=map_callback result={result} cancelled=true phase=cleanup"
        );
    }
}

fn record_queue_callback(shared: &Weak<Mutex<Signals>>) {
    if let Some(shared) = shared.upgrade() {
        let mut signals = lock_signals(&shared);
        eprintln!(
            "FS_READBACK_PROBE event=queue_callback cancelled={} phase={}",
            signals.closed,
            signals.phase.name()
        );
        if !signals.closed {
            signals.queue_callback = true;
        }
    } else {
        eprintln!("FS_READBACK_PROBE event=queue_callback cancelled=true phase=cleanup");
    }
}

async fn wait_for_external_signal(shared: &Mutex<Signals>) {
    {
        let mut signals = lock_signals(shared);
        if signals.closed {
            return;
        }
        signals.async_started = true;
        eprintln!("FS_READBACK_PROBE event=async_started");
    }
    let signalled = poll_fn(|context| {
        let mut signals = lock_signals(shared);
        if signals.closed {
            return Poll::Ready(false);
        }
        if signals.async_signal {
            return Poll::Ready(true);
        }
        signals.waker = Some(context.waker().clone());
        if !signals.async_waiting {
            signals.async_waiting = true;
            eprintln!("FS_READBACK_PROBE event=async_waiting");
        }
        Poll::Pending
    })
    .await;
    let mut signals = lock_signals(shared);
    if signalled && !signals.closed {
        signals.async_resumed = true;
        eprintln!("FS_READBACK_PROBE event=async_resumed");
    }
}

fn signal_external(shared: &Mutex<Signals>) -> Option<Waker> {
    let mut signals = lock_signals(shared);
    if signals.closed || signals.async_signal {
        return None;
    }
    let waker = signals.waker.take()?;
    signals.async_signal = true;
    eprintln!("FS_READBACK_PROBE event=async_signal");
    Some(waker)
}

fn green_pixels(bytes: &[u8]) -> bool {
    bytes.len() == BUFFER_BYTES as usize
        && (0..4).all(|y| {
            (0..4).all(|x| {
                bytes[y * ROW_PITCH as usize + x * 4..y * ROW_PITCH as usize + x * 4 + 4]
                    == [0, 255, 0, 255]
            })
        })
}

fn check_pixels(probe: &mut ActiveProbe) {
    probe
        .mapping
        .observe_callback(lock_signals(&probe.shared).map_result);
    if probe.mapping != MappingLifecycle::Mapped {
        return;
    }
    let valid = {
        let bytes = probe.buffer.slice(..).get_mapped_range();
        green_pixels(&bytes)
    }; // Release the mapped view before unmapping.
    probe.buffer.unmap();
    probe.mapping = MappingLifecycle::Unmapped;
    probe.pixels = Some(valid);
    eprintln!("FS_READBACK_PROBE event=pixels valid={valid} count=16");
}

fn finish_probe(probe: &mut ActiveProbe) {
    // Freeze evidence before unmap/destroy can deliver cancellation callbacks.
    // Release the saved waker and cancel the retained task without blocking on it.
    let mut signals = lock_signals(&probe.shared);
    signals.closed = true;
    signals.phase = Phase::Cleanup;
    signals.waker = None;
    probe.mapping.observe_callback(signals.map_result);
    drop(signals);
    probe.task = None;
    release_mapping(&mut probe.mapping, || probe.buffer.destroy());
    probe.texture.destroy();
    probe.wait = None;
    let signals = lock_signals(&probe.shared);
    let map = match signals.map_result {
        None => "missing",
        Some(true) => "ok",
        Some(false) => "error",
    };
    let pixels = match probe.pixels {
        None => "missing",
        Some(true) => "valid",
        Some(false) => "invalid",
    };
    eprintln!(
        "FS_READBACK_PROBE event=summary submitted=true queue_callback={} map_callback={map} pixels={pixels} async_started={} async_waiting={} async_signal={} async_resumed={} poll={} cleanup=true render_frames={} elapsed_ms={}",
        signals.queue_callback,
        signals.async_started,
        signals.async_waiting,
        signals.async_signal,
        signals.async_resumed,
        probe.poll_status,
        probe.render_frames,
        probe.started.elapsed().as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Wake};

    #[derive(Default)]
    struct WakeCounter(AtomicUsize);
    impl Wake for WakeCounter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Contract double for pinned wgpu-core 27: direct unmap rejects Idle,
    /// while Global::buffer_destroy handles that result internally. Pending and
    /// mapped states both become Idle when cancelled/unmapped by destroy.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum BackendMapping {
        Pending,
        Mapped,
        Idle,
    }

    #[test]
    fn cleanup_uses_destroy_for_every_mapping_state_and_delayed_callback_race() {
        let cases = [
            (MappingLifecycle::Unmapped, BackendMapping::Idle),
            (MappingLifecycle::Mapped, BackendMapping::Mapped),
            (MappingLifecycle::MapFailed, BackendMapping::Idle),
            (MappingLifecycle::AwaitingCallback, BackendMapping::Pending),
            // Backend completion/error can precede delivery of the callback.
            (MappingLifecycle::AwaitingCallback, BackendMapping::Mapped),
            (MappingLifecycle::AwaitingCallback, BackendMapping::Idle),
        ];
        for (mut owner, backend) in cases {
            let backend = Cell::new(backend);
            let destroys = Cell::new(0);
            let destroy = || {
                // This is the actual API contract in Global::buffer_destroy:
                // an internal NotMapped result is not an application error.
                let _was_mapped = backend.replace(BackendMapping::Idle) != BackendMapping::Idle;
                destroys.set(destroys.get() + 1);
            };
            release_mapping(&mut owner, destroy);
            assert_eq!(owner, MappingLifecycle::Destroyed);
            assert_eq!(backend.get(), BackendMapping::Idle);
            assert_eq!(destroys.get(), 1);
            release_mapping(&mut owner, || panic!("cleanup must be one-shot"));
            assert_eq!(destroys.get(), 1);
        }
    }

    #[test]
    fn callback_evidence_cannot_reacquire_an_unmapped_or_destroyed_mapping() {
        let mut owner = MappingLifecycle::AwaitingCallback;
        owner.observe_callback(Some(true));
        assert_eq!(owner, MappingLifecycle::Mapped);
        // check_pixels drops the view, unmaps once, then records this state.
        owner = MappingLifecycle::Unmapped;
        owner.observe_callback(Some(true));
        assert_eq!(owner, MappingLifecycle::Unmapped);
        release_mapping(&mut owner, || {});
        for late in [Some(true), Some(false), None] {
            owner.observe_callback(late);
            assert_eq!(owner, MappingLifecycle::Destroyed);
        }
        let mut failed = MappingLifecycle::AwaitingCallback;
        failed.observe_callback(Some(false));
        assert_eq!(failed, MappingLifecycle::MapFailed);
        release_mapping(&mut failed, || {});
        assert_eq!(failed, MappingLifecycle::Destroyed);
    }

    #[test]
    fn destroy_cancellation_runs_outside_lock_and_cannot_change_frozen_evidence() {
        for result in [true, false] {
            let shared = Arc::new(Mutex::new(Signals::default()));
            let weak = Arc::downgrade(&shared);
            let mut owner = MappingLifecycle::AwaitingCallback;
            {
                let mut signals = lock_signals(&shared);
                signals.closed = true;
                signals.phase = Phase::Cleanup;
                signals.waker = None;
                owner.observe_callback(signals.map_result);
            }
            release_mapping(&mut owner, || {
                assert!(shared.try_lock().is_ok());
                record_map_callback(&weak, result);
            });
            assert!(lock_signals(&shared).map_result.is_none());
            assert_eq!(owner, MappingLifecycle::Destroyed);
        }
    }

    #[test]
    fn verifies_all_sixteen_pixels_with_row_padding_and_exact_size() {
        let mut bytes = [17; BUFFER_BYTES as usize];
        for y in 0..4 {
            for x in 0..4 {
                let offset = y * ROW_PITCH as usize + x * 4;
                bytes[offset..offset + 4].copy_from_slice(&[0, 255, 0, 255]);
            }
        }
        assert!(green_pixels(&bytes));
        assert!(!green_pixels(&bytes[..bytes.len() - 1]));
        for y in 0..4 {
            for x in 0..4 {
                let offset = y * ROW_PITCH as usize + x * 4 + 1;
                bytes[offset] = 254;
                assert!(!green_pixels(&bytes));
                bytes[offset] = 255;
            }
        }
    }

    #[test]
    fn task_must_suspend_then_receive_a_real_external_wake() {
        let shared = Mutex::new(Signals::default());
        assert!(signal_external(&shared).is_none());
        let counter = Arc::new(WakeCounter::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(wait_for_external_signal(&shared));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        {
            let signals = lock_signals(&shared);
            assert!(signals.async_started && signals.async_waiting);
            assert!(!signals.async_signal && !signals.async_resumed);
        }
        let wake = signal_external(&shared).unwrap();
        // signal_external releases the lock before returning the actual waker.
        assert!(shared.try_lock().is_ok());
        wake.wake();
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert!(future.as_mut().poll(&mut cx).is_ready());
        assert!(lock_signals(&shared).async_resumed);
        assert!(signal_external(&shared).is_none());
    }

    #[test]
    fn cancellation_cannot_report_map_queue_or_async_success() {
        let shared = Arc::new(Mutex::new(Signals::default()));
        let weak = Arc::downgrade(&shared);
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut future = std::pin::pin!(wait_for_external_signal(&shared));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        {
            let mut signals = lock_signals(&shared);
            signals.closed = true;
            signals.phase = Phase::Cleanup;
            signals.waker = None;
        }
        record_map_callback(&weak, false);
        record_map_callback(&weak, true);
        record_queue_callback(&weak);
        assert!(future.as_mut().poll(&mut cx).is_ready());
        let signals = lock_signals(&shared);
        assert!(signals.map_result.is_none());
        assert!(!signals.queue_callback && !signals.async_resumed && signals.waker.is_none());
    }
}
