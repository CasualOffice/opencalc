//! When to snapshot, when to save, and what to do when saving fails.
//!
//! A state machine over supplied milliseconds. It decides; the caller performs.

/// When the session should hand the document back to the host.
///
/// The window between saves is the window in which work can be lost, so these
/// are chosen to keep it short rather than to keep traffic low.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavePolicy {
    /// Milliseconds of no edits after which to save.
    ///
    /// The cheapest moment there is: nobody is typing, so nothing is being
    /// interrupted, and the last few minutes become safe.
    pub quiesce_ms: u64,
    /// The longest a session may go without saving, however busy it is.
    ///
    /// The trigger that is easy to omit and the one that matters most under
    /// load: sustained editing never quiesces, so without a ceiling the
    /// loss window grows without bound at exactly the moment the most work is
    /// at risk.
    pub ceiling_ms: u64,
    /// Save after this many revisions regardless of time.
    pub every_revisions: u64,
    /// How many consecutive callback failures to tolerate before the document
    /// is made read-only.
    pub max_callback_attempts: u32,
    /// The first retry delay; each subsequent one doubles.
    pub retry_base_ms: u64,
}

impl Default for SavePolicy {
    fn default() -> Self {
        Self {
            quiesce_ms: 5_000,
            ceiling_ms: 60_000,
            // The same number the snapshot cadence uses: one dial rather than
            // two that drift apart.
            every_revisions: 200,
            max_callback_attempts: 6,
            retry_base_ms: 500,
        }
    }
}

/// Why a save is being asked for. Carried so the host can log it, and so a
/// test can assert *which* trigger fired rather than merely that one did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveReason {
    /// Nobody has edited for `quiesce_ms`.
    Quiesced,
    /// `ceiling_ms` has passed since the last save, and editing continues.
    Ceiling,
    /// `every_revisions` revisions have accumulated.
    Revisions,
    /// The last participant left; there is nothing to wait for.
    Closing,
    /// A previous attempt failed and its backoff has elapsed.
    Retry,
}

/// What the caller should do now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Assemble the document and POST it to the host's callback.
    Save {
        /// The revision being saved.
        revision: u64,
        /// Why.
        reason: SaveReason,
    },
    /// Tell the participants their work is not being saved.
    ///
    /// Emitted on the **first** failure, not the last: a warning is only useful
    /// while there is still time to copy the work out.
    WarnNotSaving {
        /// Which attempt just failed.
        attempt: u32,
    },
    /// Stop accepting edits, with a reason to show.
    ///
    /// Continuing to take work that provably cannot be persisted is silent loss
    /// dressed up as availability.
    GoReadOnly,
}

/// The outcome of a callback the caller performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackOutcome {
    /// The host accepted the document at this revision.
    Accepted(u64),
    /// It did not.
    Failed,
}

/// The save and callback state of one document session.
#[derive(Debug, Clone)]
pub struct SessionLifecycle {
    policy: SavePolicy,
    /// Revision the host has confirmed it holds.
    saved_revision: u64,
    /// Revision the document is at.
    revision: u64,
    /// When the last edit arrived.
    last_edit_ms: Option<u64>,
    /// When the last save was attempted, successful or not.
    last_save_ms: u64,
    /// A save is out with the caller; nothing new is asked for until it lands.
    in_flight: Option<u64>,
    /// Consecutive failures.
    failures: u32,
    /// When the next retry becomes due.
    retry_at_ms: Option<u64>,
    /// Whether the document has been made read-only.
    read_only: bool,
    /// Whether anyone is still here.
    participants: usize,
    /// Set once the last participant has gone.
    closing: bool,
}

impl SessionLifecycle {
    /// A session that has just opened at `revision`.
    #[must_use]
    pub fn new(policy: SavePolicy, revision: u64, now_ms: u64) -> Self {
        Self {
            policy,
            saved_revision: revision,
            revision,
            last_edit_ms: None,
            last_save_ms: now_ms,
            in_flight: None,
            failures: 0,
            retry_at_ms: None,
            read_only: false,
            participants: 0,
            closing: false,
        }
    }

    /// Whether edits are currently being refused.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// The revision the host is known to hold.
    #[must_use]
    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    /// Whether anything is written but not yet confirmed saved.
    #[must_use]
    pub fn has_unsaved(&self) -> bool {
        self.revision > self.saved_revision
    }

    /// Someone joined.
    pub fn joined(&mut self) {
        self.participants += 1;
        self.closing = false;
    }

    /// Someone left. The last one leaving is a save point.
    pub fn left(&mut self) {
        self.participants = self.participants.saturating_sub(1);
        if self.participants == 0 {
            self.closing = true;
        }
    }

    /// The document reached `revision`.
    pub fn committed(&mut self, revision: u64, now_ms: u64) {
        self.revision = revision;
        self.last_edit_ms = Some(now_ms);
    }

    /// What to do now, if anything.
    ///
    /// Returns at most one action per call; the caller drives it until it stops
    /// asking. Deliberately not a queue: an action changes the state it was
    /// derived from, and handing out several at once invites acting on stale
    /// ones.
    pub fn tick(&mut self, now_ms: u64) -> Option<Action> {
        if self.read_only || self.in_flight.is_some() {
            return None;
        }
        if let Some(due) = self.retry_at_ms {
            if now_ms >= due {
                return Some(self.begin_save(now_ms, SaveReason::Retry));
            }
            // Backing off: nothing else may jump the queue, or a busy document
            // would retry continuously and never back off at all.
            return None;
        }
        if !self.has_unsaved() {
            return None;
        }
        if self.closing {
            return Some(self.begin_save(now_ms, SaveReason::Closing));
        }
        if self.revision - self.saved_revision >= self.policy.every_revisions {
            return Some(self.begin_save(now_ms, SaveReason::Revisions));
        }
        if now_ms.saturating_sub(self.last_save_ms) >= self.policy.ceiling_ms {
            return Some(self.begin_save(now_ms, SaveReason::Ceiling));
        }
        if let Some(last) = self.last_edit_ms
            && now_ms.saturating_sub(last) >= self.policy.quiesce_ms
        {
            return Some(self.begin_save(now_ms, SaveReason::Quiesced));
        }
        None
    }

    fn begin_save(&mut self, now_ms: u64, reason: SaveReason) -> Action {
        self.in_flight = Some(self.revision);
        self.last_save_ms = now_ms;
        self.retry_at_ms = None;
        Action::Save {
            revision: self.revision,
            reason,
        }
    }

    /// Report what the callback did.
    ///
    /// Returns an action when the outcome demands one — the first failure
    /// warns, and exhausting the attempts stops the session.
    pub fn callback(&mut self, outcome: CallbackOutcome, now_ms: u64) -> Option<Action> {
        let attempted = self.in_flight.take()?;
        match outcome {
            CallbackOutcome::Accepted(revision) => {
                // Take the higher of the two: a host that reports a revision is
                // believed, but a stale report must never move the mark
                // backwards and re-save work already stored.
                self.saved_revision = self.saved_revision.max(revision.max(attempted));
                self.failures = 0;
                self.retry_at_ms = None;
                None
            }
            CallbackOutcome::Failed => {
                self.failures += 1;
                if self.failures >= self.policy.max_callback_attempts {
                    self.read_only = true;
                    self.retry_at_ms = None;
                    return Some(Action::GoReadOnly);
                }
                // Exponential, and saturating rather than wrapping: a long
                // outage must not fold the delay back round to nothing.
                let shift = (self.failures - 1).min(16);
                let delay = self.policy.retry_base_ms.saturating_mul(1u64 << shift);
                self.retry_at_ms = Some(now_ms.saturating_add(delay));
                if self.failures == 1 {
                    Some(Action::WarnNotSaving { attempt: 1 })
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SavePolicy {
        SavePolicy {
            quiesce_ms: 5_000,
            ceiling_ms: 60_000,
            every_revisions: 200,
            max_callback_attempts: 3,
            retry_base_ms: 500,
        }
    }

    fn open(now: u64) -> SessionLifecycle {
        let mut life = SessionLifecycle::new(policy(), 0, now);
        life.joined();
        life
    }

    #[test]
    fn an_untouched_session_never_asks_to_save() {
        let mut life = open(0);
        assert_eq!(life.tick(10_000_000), None, "nothing was edited");
    }

    #[test]
    fn quiescing_saves() {
        let mut life = open(0);
        life.committed(1, 1_000);
        assert_eq!(life.tick(4_000), None, "still within the quiet window");
        assert_eq!(
            life.tick(6_000),
            Some(Action::Save {
                revision: 1,
                reason: SaveReason::Quiesced
            })
        );
    }

    #[test]
    fn continuous_editing_still_saves_at_the_ceiling() {
        // The trigger that is easy to omit: typing every second never
        // quiesces, so without a ceiling the loss window grows without bound.
        let mut life = open(0);
        let mut now = 0;
        let mut revision = 0;
        for _ in 0..59 {
            now += 1_000;
            revision += 1;
            life.committed(revision, now);
            assert_eq!(life.tick(now), None, "never quiet, never enough revisions");
        }
        now += 1_000;
        revision += 1;
        life.committed(revision, now);
        assert!(matches!(
            life.tick(now),
            Some(Action::Save {
                reason: SaveReason::Ceiling,
                ..
            })
        ));
    }

    #[test]
    fn enough_revisions_save_without_waiting() {
        let mut life = open(0);
        life.committed(200, 100);
        assert!(matches!(
            life.tick(150),
            Some(Action::Save {
                revision: 200,
                reason: SaveReason::Revisions
            })
        ));
    }

    #[test]
    fn the_last_participant_leaving_saves_at_once() {
        let mut life = open(0);
        life.committed(3, 100);
        life.left();
        assert!(matches!(
            life.tick(150),
            Some(Action::Save {
                reason: SaveReason::Closing,
                ..
            })
        ));
    }

    #[test]
    fn only_one_save_is_in_flight_at_a_time() {
        let mut life = open(0);
        life.committed(1, 0);
        assert!(life.tick(6_000).is_some());
        assert_eq!(
            life.tick(20_000),
            None,
            "the first is still out with the caller"
        );
        life.callback(CallbackOutcome::Accepted(1), 20_000);
        assert_eq!(life.tick(20_000), None, "and it is now saved");
    }

    #[test]
    fn work_done_while_a_save_was_in_flight_is_saved_next() {
        let mut life = open(0);
        life.committed(1, 0);
        assert!(life.tick(6_000).is_some());
        life.committed(2, 6_500); // edited while the callback was out
        life.callback(CallbackOutcome::Accepted(1), 7_000);
        assert!(life.has_unsaved(), "revision 2 is not saved yet");
        assert!(matches!(
            life.tick(12_000),
            Some(Action::Save { revision: 2, .. })
        ));
    }

    #[test]
    fn the_first_failure_warns_and_later_ones_do_not() {
        let mut life = open(0);
        life.committed(1, 0);
        life.tick(6_000);
        assert_eq!(
            life.callback(CallbackOutcome::Failed, 6_000),
            Some(Action::WarnNotSaving { attempt: 1 }),
            "told while there is still time to copy the work out"
        );
        life.tick(6_500);
        assert_eq!(
            life.callback(CallbackOutcome::Failed, 6_500),
            None,
            "one warning, not one per attempt"
        );
    }

    #[test]
    fn retries_back_off_and_nothing_jumps_the_queue() {
        let mut life = open(0);
        life.committed(1, 0);
        life.tick(6_000);
        life.callback(CallbackOutcome::Failed, 6_000);

        assert_eq!(life.tick(6_100), None, "backing off");
        assert!(
            matches!(
                life.tick(6_500),
                Some(Action::Save {
                    reason: SaveReason::Retry,
                    ..
                })
            ),
            "500ms later"
        );
        life.callback(CallbackOutcome::Failed, 6_500);
        assert_eq!(life.tick(6_900), None, "now a second of backoff");
        assert!(life.tick(7_500).is_some());
    }

    #[test]
    fn exhausting_the_attempts_stops_the_session() {
        let mut life = open(0);
        life.committed(1, 0);
        let mut now = 6_000;
        life.tick(now);
        life.callback(CallbackOutcome::Failed, now);
        now += 500;
        life.tick(now);
        life.callback(CallbackOutcome::Failed, now);
        now += 1_000;
        life.tick(now);
        assert_eq!(
            life.callback(CallbackOutcome::Failed, now),
            Some(Action::GoReadOnly),
            "three attempts is the configured limit"
        );
        assert!(life.is_read_only());
        assert_eq!(
            life.tick(now + 10_000_000),
            None,
            "and it stays stopped rather than quietly resuming"
        );
    }

    #[test]
    fn a_success_after_failures_clears_the_backoff() {
        let mut life = open(0);
        life.committed(1, 0);
        life.tick(6_000);
        life.callback(CallbackOutcome::Failed, 6_000);
        life.tick(6_500);
        life.callback(CallbackOutcome::Accepted(1), 6_500);

        life.committed(2, 7_000);
        assert!(
            matches!(
                life.tick(12_100),
                Some(Action::Save {
                    reason: SaveReason::Quiesced,
                    ..
                })
            ),
            "back to normal cadence, not still retrying"
        );
    }

    #[test]
    fn a_stale_acknowledgement_never_moves_the_mark_backwards() {
        // A host that reports an older revision must not cause work already
        // stored to be re-sent, or a slow duplicate reply would loop forever.
        let mut life = open(0);
        life.committed(5, 0);
        life.tick(6_000);
        life.callback(CallbackOutcome::Accepted(5), 6_000);
        assert_eq!(life.saved_revision(), 5);

        life.committed(9, 7_000);
        life.tick(13_000);
        life.callback(CallbackOutcome::Accepted(2), 13_000);
        assert_eq!(life.saved_revision(), 9, "the attempted revision wins");
        assert!(!life.has_unsaved());
    }
}
