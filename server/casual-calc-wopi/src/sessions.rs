//! What this service remembers between opening a file and saving it.
//!
//! A WOPI access token belongs to one file, one user and one host, and it
//! expires. The collaboration server must not be given it — it would end up in
//! that server's configuration, its logs, and its cluster log — so the adapter
//! keeps it and proxies both legs itself: the server fetches from *us* and
//! saves to *us*, and only this process ever holds the host's credential.
//!
//! # The session id is a capability
//!
//! Anyone who knows one can read and overwrite that file for as long as the
//! session lives. So it is 256 bits from the operating system's random source,
//! not a counter and not a hash of the clock — the demo host's `uuid()` says in
//! its own doc comment that it is "enough for a demo", and this is not one.
//!
//! # Sessions are bounded and they expire
//!
//! Every one holds a lock on somebody's file. A map that only grows is a
//! process that eventually dies holding every lock it ever took, and a WOPI
//! host has no way to tell that the editor is gone rather than busy.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::wopi::FileInfo;

/// One open file.
#[derive(Debug, Clone)]
pub struct Session {
    /// The `WOPISrc` the host gave us.
    pub src: String,
    /// The host's access token. Never logged, never sent onward.
    pub token: String,
    /// The lock id held on the file, if the host supports locking.
    pub lock: Option<String>,
    /// The filename, for the editor's title bar.
    pub title: String,
    /// Whether this user may write. Derived from `UserCanWrite`, and from
    /// whether the host will accept a `PutFile` at all.
    pub editable: bool,
    /// Who is editing, for the presence roster.
    pub user_name: String,
    /// A stable id for them.
    pub user_id: String,
    /// When it was opened, as Unix milliseconds.
    pub opened_ms: u64,
    /// When the lock was last refreshed, as Unix milliseconds.
    pub refreshed_ms: u64,
}

impl Session {
    /// Build a session from what `CheckFileInfo` said.
    #[must_use]
    pub fn from(src: String, token: String, info: &FileInfo, now_ms: u64) -> Self {
        Self {
            src,
            token,
            lock: None,
            title: info.base_file_name.clone(),
            // Both, not either: a host that says the user may write but does
            // not implement `PutFile` cannot be saved to, and finding that out
            // at the end of an hour's editing is the worst possible moment.
            editable: info.user_can_write && info.supports_update,
            user_name: info
                .user_friendly_name
                .clone()
                .unwrap_or_else(|| "Guest".to_owned()),
            user_id: info.user_id.clone().unwrap_or_else(|| fresh_id(8)),
            opened_ms: now_ms,
            refreshed_ms: now_ms,
        }
    }
}

/// Every open session on this node.
#[derive(Debug)]
pub struct Sessions {
    open: Mutex<HashMap<String, Session>>,
    limit: usize,
    ttl_ms: u64,
}

impl Sessions {
    /// A registry holding at most `limit` sessions, each for at most `ttl_ms`.
    #[must_use]
    pub fn new(limit: usize, ttl_ms: u64) -> Self {
        Self {
            open: Mutex::new(HashMap::new()),
            limit,
            ttl_ms,
        }
    }

    /// Take a session, returning the id that addresses it.
    ///
    /// # Errors
    ///
    /// The session back, unchanged, if this node is already holding as many as
    /// it will. Returning it rather than dropping it lets the caller unlock the
    /// file it has just locked — dropping it would leave that lock held by a
    /// session nobody can reach.
    pub fn insert(&self, session: Session, now_ms: u64) -> Result<String, Box<Session>> {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        open.retain(|_, s| now_ms.saturating_sub(s.opened_ms) < self.ttl_ms);
        if open.len() >= self.limit {
            return Err(Box::new(session));
        }
        let id = fresh_id(32);
        open.insert(id.clone(), session);
        Ok(id)
    }

    /// The session `id` names, if it is still open.
    #[must_use]
    pub fn get(&self, id: &str, now_ms: u64) -> Option<Session> {
        let open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        open.get(id)
            .filter(|s| now_ms.saturating_sub(s.opened_ms) < self.ttl_ms)
            .cloned()
    }

    /// Record the lock taken for a session.
    pub fn set_lock(&self, id: &str, lock: Option<String>, now_ms: u64) {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(session) = open.get_mut(id) {
            session.lock = lock;
            session.refreshed_ms = now_ms;
        }
    }

    /// Forget a session, returning it so its lock can be released.
    pub fn remove(&self, id: &str) -> Option<Session> {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        open.remove(id)
    }

    /// Every session whose lock is due a refresh, with its id.
    ///
    /// WOPI locks expire after 30 minutes. This is on a timer rather than on
    /// activity deliberately: a document left open over lunch is exactly the
    /// one whose lock must survive, and tying the refresh to keystrokes loses
    /// the lock precisely when nothing is happening.
    #[must_use]
    pub fn due_for_refresh(&self, now_ms: u64, every_ms: u64) -> Vec<(String, Session)> {
        let open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        open.iter()
            .filter(|(_, s)| s.lock.is_some())
            .filter(|(_, s)| now_ms.saturating_sub(s.refreshed_ms) >= every_ms)
            .filter(|(_, s)| now_ms.saturating_sub(s.opened_ms) < self.ttl_ms)
            .map(|(id, s)| (id.clone(), s.clone()))
            .collect()
    }

    /// Every session that has aged out, removed, so their locks can be released.
    #[must_use]
    pub fn take_expired(&self, now_ms: u64) -> Vec<Session> {
        let mut open = self.open.lock().unwrap_or_else(|p| p.into_inner());
        let dead: Vec<String> = open
            .iter()
            .filter(|(_, s)| now_ms.saturating_sub(s.opened_ms) >= self.ttl_ms)
            .map(|(id, _)| id.clone())
            .collect();
        dead.iter().filter_map(|id| open.remove(id)).collect()
    }

    /// How many are open. For the health endpoint and for tests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.open.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

/// `bytes` bytes of operating-system randomness, hex encoded.
///
/// # Panics
///
/// If the operating system cannot produce randomness, which is not a condition
/// this service can carry on through: every id it mints after that would be a
/// capability somebody can guess.
#[must_use]
pub fn fresh_id(bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    getrandom::fill(&mut raw).expect("the operating system's random source");
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests;
