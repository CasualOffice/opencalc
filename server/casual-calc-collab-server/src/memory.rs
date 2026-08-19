//! Admission by memory pressure, not by counting.
//!
//! A node used to refuse the thousand-and-first document and the
//! two-hundred-and-first participant, whatever it was actually using. Those
//! numbers were a guess at what a machine could hold, and a guess is wrong in
//! both directions at once: a node with 64 GB turned work away at 8 GB, and a
//! node with 2 GB accepted a thousand documents it could not hold.
//!
//! So the gate is the resource itself. Documents and participants are unbounded
//! until the node is genuinely short of memory, and then admission stops.
//!
//! # Why it refuses *before* exhaustion, not at it
//!
//! "Only when resources run out" cannot mean waiting for the allocator to fail.
//! On Linux a process that reaches its cgroup limit is killed, and an OOM kill
//! takes **every document on the node**, including the unsaved work of everybody
//! in them. Refusing one new document loses one person a click; being killed
//! loses everybody their afternoon.
//!
//! So there is a high-water mark below the real ceiling. It is not a cap on
//! work — it is the point at which this node says "not me" and, in a cluster,
//! somebody else takes the document.
//!
//! # Why current usage and not the peak
//!
//! `PERF-10` measures `VmHWM`, the high-water mark, because "will this fit" is
//! a question about the worst moment. Admission asks a different question —
//! "is there room *now*" — and must use `VmRSS`. A peak never falls, so a node
//! that once touched the mark would refuse for ever, including after everything
//! was evicted.

/// What the node may use before it stops admitting work.
#[derive(Debug, Clone, Copy)]
pub struct MemoryBudget {
    /// The ceiling this node is subject to, in bytes.
    pub limit_bytes: u64,
    /// The fraction of it at which admission stops, in percent.
    pub high_water_percent: u64,
}

impl MemoryBudget {
    /// The byte figure admission compares against.
    #[must_use]
    pub const fn high_water_bytes(&self) -> u64 {
        self.limit_bytes / 100 * self.high_water_percent
    }

    /// Whether `used` leaves room to take on more work.
    #[must_use]
    pub const fn admits(&self, used: u64) -> bool {
        used < self.high_water_bytes()
    }
}

/// The memory ceiling this process is actually subject to.
///
/// The container's limit, not the machine's: a node with a 2 GB cgroup limit on
/// a 64 GB host is a 2 GB node, and sizing from the host is how a container gets
/// killed while `free` looks healthy.
///
/// cgroup v2 first, then v1, because a v2 host still carries v1 paths on some
/// distributions and the v2 answer is the operative one where both exist.
#[must_use]
pub fn container_limit_bytes() -> Option<u64> {
    let v2 = std::fs::read_to_string("/sys/fs/cgroup/memory.max").ok();
    if let Some(text) = v2 {
        return parse_cgroup_limit(&text);
    }
    let v1 = std::fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes").ok()?;
    parse_cgroup_limit(&v1)
}

/// Read a cgroup limit file's body.
///
/// `max` means unbounded in v2. v1 spells the same thing as a number so large
/// it is meaningless — near `u64::MAX` — and treating that as a real ceiling
/// would set a high-water mark no process could ever reach, which is a limit
/// that silently does nothing.
#[must_use]
pub fn parse_cgroup_limit(text: &str) -> Option<u64> {
    let text = text.trim();
    if text == "max" {
        return None;
    }
    let value: u64 = text.parse().ok()?;
    // Anything at or above a petabyte is v1's way of saying "no limit".
    if value >= 1 << 50 { None } else { Some(value) }
}

/// Resident set size now, in bytes.
///
/// `VmRSS`, not `VmHWM` — see the module note.
#[must_use]
pub fn current_resident_bytes() -> Option<u64> {
    parse_vm_rss(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// Pull `VmRSS` out of a `/proc/self/status` body, in bytes.
///
/// Split from the read so it is testable on a machine with no `/proc`, which is
/// every developer machine here.
#[must_use]
pub fn parse_vm_rss(status: &str) -> Option<u64> {
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

/// Whether this node can take on more work.
///
/// `None` for the budget or the reading means **admit**: a platform this cannot
/// measure must not be turned into a node that refuses everything. The operator
/// is warned at boot instead, because an unmeasurable node with no count cap is
/// genuinely unprotected and that is a thing to know rather than discover.
#[must_use]
pub fn admits(budget: Option<MemoryBudget>, used: Option<u64>) -> bool {
    match (budget, used) {
        (Some(budget), Some(used)) => budget.admits(used),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The check that runs under a held lock must not take one.**
    ///
    /// This is the shape of the bug that reached main: the admission check
    /// locked the document registry to count, and one caller already held that
    /// lock, so the server deadlocked on the first join. It did not crash and
    /// logged nothing — the socket simply went quiet for twenty seconds and
    /// then for ever, which is the worst way for a server to fail because
    /// nothing anywhere says what happened.
    ///
    /// Asserted with a timeout rather than by inspection, because a deadlock is
    /// invisible to every other kind of test: it does not fail, it waits.
    #[test]
    fn the_memory_check_completes_while_a_lock_is_held() {
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::Duration;

        // Stands in for `registry.live`: a caller holds it and then asks
        // whether there is room.
        let held: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let guard = held.lock().unwrap();

        let (tx, rx) = mpsc::channel();
        let budget = MemoryBudget {
            limit_bytes: 1_000_000_000,
            high_water_percent: 85,
        };
        std::thread::spawn(move || {
            // Exactly what `admits_memory` does: read usage, compare. No lock.
            let answer = admits(Some(budget), current_resident_bytes());
            let _ = tx.send(answer);
        });

        let answered = rx.recv_timeout(Duration::from_secs(5));
        drop(guard);
        assert!(
            answered.is_ok(),
            "the memory check did not answer within five seconds while a lock was held \
             — it is acquiring something, which is the deadlock this guards"
        );
    }

    /// **A budget refuses before the ceiling, not at it.**
    ///
    /// The whole point: reaching the cgroup limit is an OOM kill, which takes
    /// every document on the node. Admission has to stop while there is still
    /// room to keep working.
    #[test]
    fn admission_stops_below_the_ceiling() {
        let budget = MemoryBudget {
            limit_bytes: 1_000_000_000,
            high_water_percent: 85,
        };
        assert_eq!(budget.high_water_bytes(), 850_000_000);
        assert!(budget.admits(800_000_000), "refused with 20% still free");
        assert!(
            !budget.admits(900_000_000),
            "admitted past the high-water mark"
        );
        assert!(
            !budget.admits(999_999_999),
            "admitted at the very ceiling, which is where the kernel intervenes"
        );
    }

    /// **An unmeasurable node admits rather than refuses everything.**
    ///
    /// A node that cannot read its own usage must not become one that turns
    /// every document away — that would take a machine with no cgroup and no
    /// `/proc` from unprotected to useless.
    #[test]
    fn an_unmeasurable_node_admits() {
        let budget = MemoryBudget {
            limit_bytes: 1_000,
            high_water_percent: 85,
        };
        assert!(admits(None, Some(u64::MAX)), "no budget must not refuse");
        assert!(admits(Some(budget), None), "no reading must not refuse");
        assert!(admits(None, None));
        // But with both, the budget decides.
        assert!(!admits(Some(budget), Some(999)));
    }

    /// **`max` and v1's enormous sentinel both mean no limit.**
    ///
    /// v1 spells "unlimited" as a number near `u64::MAX`. Taking it literally
    /// sets a high-water mark no process could reach — a limit that silently
    /// does nothing, which is worse than none because it looks configured.
    #[test]
    fn an_unlimited_cgroup_is_read_as_no_limit() {
        assert_eq!(parse_cgroup_limit("max"), None);
        assert_eq!(parse_cgroup_limit("max\n"), None);
        assert_eq!(parse_cgroup_limit("9223372036854771712"), None);
        assert_eq!(parse_cgroup_limit("2147483648"), Some(2_147_483_648));
        assert_eq!(parse_cgroup_limit("  2147483648\n"), Some(2_147_483_648));
        assert_eq!(parse_cgroup_limit("not a number"), None);
    }

    /// **`VmRSS`, not `VmHWM`.**
    ///
    /// Admission asks "is there room now"; a peak never falls, so a node that
    /// once touched the mark would refuse for ever — including after everything
    /// was evicted. `PERF-10` reads the peak for the opposite reason.
    #[test]
    fn the_current_size_is_read_not_the_peak() {
        let status = "\
Name:\tcollab
VmPeak:\t  812345 kB
VmHWM:\t   524288 kB
VmRSS:\t   131072 kB
";
        assert_eq!(parse_vm_rss(status), Some(131_072 * 1024));
        assert_ne!(parse_vm_rss(status), Some(524_288 * 1024), "read the peak");
    }

    /// A platform whose status carries no such field is unmeasurable.
    #[test]
    fn a_status_without_the_field_is_unmeasurable() {
        assert_eq!(parse_vm_rss("Name:\tcollab\n"), None);
        assert_eq!(parse_vm_rss(""), None);
    }
}
