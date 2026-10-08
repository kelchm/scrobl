//! The minimum spacing between request starts, shared by every clone of a
//! client.

use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::{Instant, sleep};

/// The longest interval a client accepts. It keeps every sum of an interval
/// and a clock reading far inside what an `Instant` can hold.
pub(super) const MAX_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Admits callers at least `interval` apart.
///
/// Admission is serialised and measured when it happens. A caller takes the
/// lock, which Tokio hands out in the order callers asked for it, works out
/// how much of the interval is left since the previous admission, sleeps
/// that long *while holding the lock*, and then reads the clock again to
/// record the time it was actually admitted. The next caller measures from
/// that time. So the spacing holds however late a task is polled: a runtime
/// that stalls past several intervals delays the callers, it does not let
/// them through together.
///
/// Nothing is reserved ahead. A caller that is dropped, whether it is queued
/// for the lock or asleep under it, has recorded nothing and leaves nothing
/// spent: the caller behind it waits only for what remains of the interval
/// since the last real admission.
#[derive(Debug)]
pub(super) struct Pacer {
    interval: Duration,
    /// When the last caller was admitted, if any was.
    last: Mutex<Option<Instant>>,
}

impl Pacer {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: Mutex::new(None),
        }
    }

    /// Waits until a request may start, and returns the time it was
    /// admitted. Returns at once when pacing is off.
    ///
    /// The wait is worked out as a `Duration`, never as an `Instant` plus an
    /// interval, so no interval can overflow the clock.
    pub(super) async fn wait(&self) -> Instant {
        if self.interval.is_zero() {
            return Instant::now();
        }
        let mut last = self.last.lock().await;
        if let Some(previous) = *last
            && let Some(remaining) = self.interval.checked_sub(previous.elapsed())
        {
            sleep(remaining).await;
        }
        // Read again: the sleep may have ended late. Dropping this future
        // before this line records nothing.
        let admitted = Instant::now();
        *last = Some(admitted);
        admitted
    }
}
