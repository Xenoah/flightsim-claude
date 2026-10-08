//! Diagnostic-only, single-capture recorder. Never include in a qualifying build.
//! Writers perform fixed atomic stores and a monotonic clock read, with no locks,
//! allocation, formatting, file I/O, GPU work, polling, waiting or task spawning.
//! The existing main-world screenshot system is the sole reader and stderr sink.
#![forbid(unsafe_code)]

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

pub const CAPACITY: usize = 2048;

struct Slot {
    ready: AtomicBool,
    event: AtomicU64,
    us: AtomicU64,
    a: AtomicU64,
    b: AtomicU64,
    c: AtomicU64,
}

impl Slot {
    const fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            event: AtomicU64::new(0),
            us: AtomicU64::new(0),
            a: AtomicU64::new(0),
            b: AtomicU64::new(0),
            c: AtomicU64::new(0),
        }
    }
}

static START: OnceLock<Instant> = OnceLock::new();
static SLOTS: [Slot; CAPACITY] = [const { Slot::new() }; CAPACITY];
static NEXT: AtomicUsize = AtomicUsize::new(0);
static DRAINED: AtomicUsize = AtomicUsize::new(0);
static DROPPED: AtomicUsize = AtomicUsize::new(0);
static LAST_DROPPED: AtomicUsize = AtomicUsize::new(0);
static LAST_RESERVED: AtomicUsize = AtomicUsize::new(0);
static IO_ERRORS: AtomicUsize = AtomicUsize::new(0);
static BUFFER: AtomicUsize = AtomicUsize::new(0);
static DEVICE: AtomicUsize = AtomicUsize::new(0);
static SELECTED_ONCE: AtomicBool = AtomicBool::new(false);
static ENTITY: AtomicU64 = AtomicU64::new(u64::MAX);

/// The first encoded screenshot owns the trace. Subsequent screenshots are
/// excluded; the runner additionally permits exactly one screenshot invocation.
pub fn bevy(event: u64, entity: u64, b: u64, c: u64) {
    let _ = ENTITY.compare_exchange(u64::MAX, entity, Ordering::AcqRel, Ordering::Acquire);
    if ENTITY.load(Ordering::Acquire) == entity {
        record(event, entity, b, c);
    }
}

/// Called before Bevy creates its first screenshot buffer, outside engine locks.
pub fn begin() {
    if START.set(Instant::now()).is_ok() {
        record(1, 0, 0, 0);
    }
}

/// Select exactly the first real screenshot buffer. Addresses remain internal.
pub fn select(buffer: usize, device: usize, index: u64, epoch: u64) -> bool {
    if START.get().is_none() || buffer == 0 {
        return false;
    }
    if SELECTED_ONCE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        DEVICE.store(device, Ordering::Release);
        BUFFER.store(buffer, Ordering::Release);
        record(20, index, epoch, 0);
        true
    } else {
        is_buffer(buffer)
    }
}

pub fn is_buffer(buffer: usize) -> bool {
    buffer != 0 && BUFFER.load(Ordering::Acquire) == buffer
}

pub fn is_device(device: usize) -> bool {
    BUFFER.load(Ordering::Acquire) != 0 && DEVICE.load(Ordering::Acquire) == device
}

/// Called at the original Buffer::drop, without retaining another resource Arc.
/// The selector is never reused, so recycled addresses cannot produce evidence.
pub fn release(buffer: usize) {
    if is_buffer(buffer) {
        record(39, 0, 0, 0);
        BUFFER.store(0, Ordering::Release);
        DEVICE.store(0, Ordering::Release);
    }
}

pub fn record(event: u64, a: u64, b: u64, c: u64) {
    let Some(start) = START.get() else { return };
    // Each slot has one writer, is published once, and is never recycled.
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let Some(slot) = SLOTS.get(seq) else {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    };
    slot.us
        .store(start.elapsed().as_micros() as u64, Ordering::Relaxed);
    slot.event.store(event, Ordering::Relaxed);
    slot.a.store(a, Ordering::Relaxed);
    slot.b.store(b, Ordering::Relaxed);
    slot.c.store(c, Ordering::Relaxed);
    slot.ready.store(true, Ordering::Release);
}

/// Main-world only, before acquiring / after dropping the screenshot receiver
/// mutex. At most 64 complete records per call. Never wait for an unpublished
/// slot: the next existing frame can drain it. A stopped main loop can leave an
/// undrained tail; absence of a line is never proof that an event did not occur.
pub fn drain() {
    if START.get().is_none() {
        return;
    }
    let stderr = std::io::stderr();
    let mut sink = stderr.lock();
    drain_to(&mut sink);
}

fn drain_to(sink: &mut impl Write) {
    let before = DRAINED.load(Ordering::Relaxed);
    let mut next = before;
    for slot in SLOTS.iter().skip(before).take(64) {
        if !slot.ready.load(Ordering::Acquire) {
            break;
        }
        if writeln!(
            sink,
            "FS_READBACK_TRACE schema=1 seq={} us={} event={} a={} b={} c={}",
            next,
            slot.us.load(Ordering::Relaxed),
            event_name(slot.event.load(Ordering::Relaxed)),
            slot.a.load(Ordering::Relaxed),
            slot.b.load(Ordering::Relaxed),
            slot.c.load(Ordering::Relaxed),
        )
        .is_err()
        {
            IO_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        next += 1;
    }
    DRAINED.store(next, Ordering::Relaxed);
    let dropped = DROPPED.load(Ordering::Relaxed);
    let prior_dropped = LAST_DROPPED.swap(dropped, Ordering::Relaxed);
    let reserved = NEXT.load(Ordering::Relaxed);
    let prior_reserved = LAST_RESERVED.swap(reserved, Ordering::Relaxed);
    // Report the first overflow; do not emit unlimited status lines after the
    // fixed buffer fills. Counts are explicitly snapshots / lower bounds.
    if next != before
        || (dropped > 0 && prior_dropped == 0)
        || (reserved != prior_reserved && reserved <= CAPACITY)
    {
        let io_errors = IO_ERRORS.load(Ordering::Relaxed);
        let _ = writeln!(sink, "FS_READBACK_TRACE_STATUS schema=1 reserved={reserved} drained={next} dropped={dropped} io_errors={io_errors} capacity={CAPACITY}");
    }
}

fn event_name(id: u64) -> &'static str {
    match id {
        1 => "capture_prepare",
        2 => "copy_encode",
        3 => "task_spawn",
        4 => "task_enter",
        5 => "map_register_enter",
        6 => "map_register_return",
        7 => "bevy_callback_enter",
        8 => "channel_send_enter",
        9 => "channel_send_return",
        10 => "receive_resumed",
        11 => "mapped_range_enter",
        12 => "mapped_range_return",
        13 => "cpu_copy_return",
        14 => "image_send_enter",
        15 => "image_send_return",
        16 => "main_receive",
        17 => "event_queued",
        20 => "buffer_selected",
        21 => "core_map_enter",
        22 => "core_map_return",
        23 => "core_map_error",
        24 => "callback_invoke",
        25 => "callback_return",
        26 => "dependency_assigned",
        27 => "pending_promoted",
        28 => "handle_map_enter",
        29 => "handle_map_return",
        30 => "buffer_map_enter",
        31 => "map_state_waiting",
        32 => "map_state_idle",
        33 => "map_state_active",
        34 => "backend_map_enter",
        35 => "backend_map_return",
        36 => "buffer_map_return",
        37 => "fence_result",
        38 => "maintain_return",
        39 => "buffer_drop",
        40 => "fence_enter",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_publication_is_complete_bounded_and_does_not_reselect() {
        // A real multiproducer record/publication test. No engine or GPU involved.
        begin();
        assert!(select(10, 11, 12, 13));
        assert!(!select(20, 21, 22, 23));
        // Reservation 2 is deliberately unpublished while 3 is complete.
        let gap = NEXT.fetch_add(1, Ordering::Relaxed);
        assert_eq!(gap, 2);
        record(2, 99, 0, 99000);
        let mut output = Vec::new();
        drain_to(&mut output);
        assert_eq!(DRAINED.load(Ordering::Relaxed), 2);
        assert!(!String::from_utf8(output).unwrap().contains("seq=3 "));
        SLOTS[gap].event.store(2, Ordering::Relaxed);
        SLOTS[gap].ready.store(true, Ordering::Release);
        drain_to(&mut Vec::new());
        assert_eq!(DRAINED.load(Ordering::Relaxed), 4);
        struct BrokenSink;
        impl Write for BrokenSink {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        record(2, 0, 0, 0);
        drain_to(&mut BrokenSink);
        assert_eq!(DRAINED.load(Ordering::Relaxed), 5);
        assert_eq!(IO_ERRORS.load(Ordering::Relaxed), 1);
        std::thread::scope(|scope| {
            for producer in 1..=8_u64 {
                scope.spawn(move || {
                    for row in 0..400_u64 {
                        record(2, producer, row, producer * 1000 + row);
                    }
                });
            }
        });
        assert_eq!(NEXT.load(Ordering::Relaxed), 3205);
        assert_eq!(DROPPED.load(Ordering::Relaxed), 3205 - CAPACITY);
        for slot in &SLOTS[5..] {
            assert!(slot.ready.load(Ordering::Acquire));
            let a = slot.a.load(Ordering::Relaxed);
            let b = slot.b.load(Ordering::Relaxed);
            assert_eq!(slot.c.load(Ordering::Relaxed), a * 1000 + b);
        }
        release(10);
        assert!(!is_buffer(10));
        assert!(!is_device(11));
        assert!(!select(10, 11, 12, 13));
    }
}
