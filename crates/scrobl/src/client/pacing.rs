//! The minimum spacing between request starts, shared by every clone of a
//! client.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::{Instant, sleep_until};

/// Hands out start times at least `interval` apart.
///
/// A caller reserves its slot under the lock, which only does arithmetic,
/// and then waits for the slot with the lock released. Callers therefore
/// queue behind one another: each reservation starts where the previous one
/// ended, so a caller that is waiting never moves the clock for the others.
/// A caller that is dropped while waiting has spent its slot, and the slots
/// after it keep their times.
#[derive(Debug)]
pub(super) struct Pacer {
    interval: Duration,
    /// The earliest time the next request may start.
    next: Mutex<Instant>,
}

impl Pacer {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: Mutex::new(Instant::now()),
        }
    }

    /// Reserves the next slot and returns when it starts. Does no waiting.
    pub(super) fn reserve(&self) -> Instant {
        let now = Instant::now();
        // The guarded value is a plain `Instant`, so a poisoned lock holds
        // no half-made state.
        let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = (*next).max(now);
        *next = slot + self.interval;
        slot
    }

    /// Waits for a slot. Returns at once when pacing is off.
    pub(super) async fn wait(&self) {
        if self.interval.is_zero() {
            return;
        }
        let slot = self.reserve();
        if slot > Instant::now() {
            sleep_until(slot).await;
        }
    }
}
