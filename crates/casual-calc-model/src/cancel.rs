//! Stopping a long job that has already started.
//!
//! docs/07 and docs/21 both promise that admission and full recalculation are
//! "bounded *and* cancellable". They were bounded. Nothing in any crate could
//! stop one (`SEC-012`), so a workbook that was inside every limit and simply
//! enormous held the only thread the browser has until it finished.
//!
//! # Cooperative, because nothing else is possible here
//!
//! The engine runs in a browser, on one thread. There is no signal to deliver,
//! no other thread to raise a flag from mid-loop, and no pre-emption available.
//! A job can only stop by **asking**, so the shape of this is a question the
//! long loops put periodically rather than a thing done to them.
//!
//! That is a constraint, not a preference: a loop that never asks cannot be
//! cancelled however good the token is, which is why the checks live inside the
//! loops that are actually long rather than at their edges.
//!
//! # Why there is no deadline type here
//!
//! The obvious token is a wall-clock deadline, and this crate cannot build one:
//! `std::time::Instant::now` **panics** on `wasm32-unknown-unknown`, which is
//! the target that needs cancellation most. So the clock belongs to the host,
//! and [`Cancel`] is implemented for any `Fn() -> bool` — a browser host passes
//! a closure over `performance.now()`, a native one over `Instant`, and neither
//! has to be described here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Something a long job asks, now and then, whether it should stop.
pub trait Cancel {
    /// Whether the caller has asked for this job to stop.
    ///
    /// Called often — every few thousand units of work — so it must be cheap
    /// and must not block.
    fn cancelled(&self) -> bool;
}

/// Nothing ever cancels. The default, and free.
#[derive(Debug, Clone, Copy, Default)]
pub struct Never;

impl Cancel for Never {
    fn cancelled(&self) -> bool {
        false
    }
}

/// Any predicate is a cancellation token.
///
/// This is what keeps the clock out of this crate: a browser host closes over
/// `performance.now()`, a native one over `Instant`, and a test over a counter.
impl<F: Fn() -> bool> Cancel for F {
    fn cancelled(&self) -> bool {
        self()
    }
}

/// A flag a host can raise from anywhere, including another thread.
///
/// Cloning shares the flag; raising it on any clone stops every job holding
/// one.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// A flag that has not been raised.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask every job holding this flag to stop.
    pub fn cancel(&self) {
        // `Release`/`Acquire` rather than `Relaxed`: the point of raising this
        // is that the job stops *and* sees whatever the canceller wrote before
        // raising it — a reason string, a request id — and relaxed ordering
        // gives no guarantee about the second.
        self.0.store(true, Ordering::Release);
    }

    /// Whether it has been raised.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl Cancel for CancelFlag {
    fn cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

/// How often a loop should ask.
///
/// Asking on every cell costs a virtual call per cell on a path that is
/// otherwise a few instructions, and is the kind of overhead that turns up in
/// the T1 benchmark rather than in review. Asking every few thousand bounds the
/// delay between a request and a stop to something no person can perceive,
/// while making the check free in the profile.
pub const CANCEL_CHECK_INTERVAL: usize = 4096;

/// Whether a job at `progress` units should ask now.
///
/// `progress` counts from one, so the first check happens after a full interval
/// rather than immediately — a job that has done nothing has nothing to stop.
#[must_use]
pub fn should_check(progress: usize) -> bool {
    progress.is_multiple_of(CANCEL_CHECK_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_stops_every_holder() {
        let flag = CancelFlag::new();
        let held = flag.clone();
        assert!(!flag.cancelled());
        assert!(!held.cancelled());

        held.cancel();
        assert!(flag.cancelled(), "the flag is shared, not copied");
        assert!(held.cancelled());
    }

    #[test]
    fn never_never_does() {
        assert!(!Never.cancelled());
    }

    /// **A predicate is a token**, which is how a browser host supplies a clock
    /// this crate cannot build.
    #[test]
    fn a_closure_is_a_token() {
        // `Fn`, not `FnMut`, so the state it consults lives outside it — which
        // is also how a host holds a clock.
        let asks = std::cell::Cell::new(0);
        let token = || {
            asks.set(asks.get() + 1);
            asks.get() > 3
        };
        assert!(!token.cancelled());
        assert!(!token.cancelled());
        assert!(!token.cancelled());
        assert!(token.cancelled(), "the fourth ask stops it");
    }

    /// **Checks are periodic, and the first one is not immediate.**
    #[test]
    fn the_check_interval_is_periodic() {
        assert!(!should_check(1));
        assert!(!should_check(CANCEL_CHECK_INTERVAL - 1));
        assert!(should_check(CANCEL_CHECK_INTERVAL));
        assert!(should_check(CANCEL_CHECK_INTERVAL * 2));
        assert!(!should_check(CANCEL_CHECK_INTERVAL + 1));
    }
}
