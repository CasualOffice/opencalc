//! Who is here, and where they are looking.
//!
//! Deliberately the shallowest thing in this crate. Presence is **not part of
//! the document**: it is never transformed, never persisted, never in a
//! snapshot, and losing all of it costs nothing — which is exactly why it is
//! allowed to take the cheap path that the document is not.
//!
//! The shape is [Yjs's awareness
//! protocol](https://docs.yjs.dev/getting-started/adding-awareness): a map
//! keyed by participant, each owning exactly one entry that it overwrites
//! wholesale, dropped after a period of silence. No merge, so no transform.
//!
//! # Identity comes from the token
//!
//! A participant's name and colour are recorded when it joins, from the
//! host-signed token, and a presence update carries only *where* it is looking.
//! Presence is the one surface where a claimed identity would be believed
//! without question, so the client is never asked for one — and
//! [`ClientMessage::Presence`](casual_calc_transaction::protocol::ClientMessage)
//! has no field it could put one in.

use std::collections::BTreeMap;

use casual_calc_transaction::session::ClientId;

/// How long a participant may go unheard before it is presumed gone.
///
/// Yjs's thirty seconds, which is roughly three missed heartbeats at a
/// ten-second interval. Adopted rather than invented: the number has to
/// tolerate a slow network without leaving ghosts on screen for a minute, and
/// this one is known to.
pub const DEFAULT_TTL_MS: u64 = 30_000;

/// What another participant sees of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    /// Display name, from the token.
    pub name: String,
    /// Colour, from the token or derived from the id.
    pub color: String,
    /// The sheet being viewed.
    pub sheet: usize,
    /// The selection, as `[r0, c0, r1, c1]`.
    pub selection: [u32; 4],
}

#[derive(Debug, Clone)]
struct Entry {
    presence: Presence,
    seen_ms: u64,
}

/// The participants of one document.
#[derive(Debug, Clone)]
pub struct Roster {
    ttl_ms: u64,
    entries: BTreeMap<ClientId, Entry>,
}

impl Default for Roster {
    fn default() -> Self {
        Self::new(DEFAULT_TTL_MS)
    }
}

impl Roster {
    /// A roster that forgets a participant after `ttl_ms` of silence.
    #[must_use]
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            ttl_ms,
            entries: BTreeMap::new(),
        }
    }

    /// Register a participant, with the identity its token gave it.
    ///
    /// An absent colour is derived from the id rather than picked at random, so
    /// somebody looks the same in every session instead of changing hue each
    /// time they rejoin — which is the difference between a colour that
    /// identifies a person and one that merely distinguishes cursors.
    pub fn joined(
        &mut self,
        client: ClientId,
        name: impl Into<String>,
        color: Option<String>,
        now_ms: u64,
    ) {
        self.entries.insert(
            client,
            Entry {
                presence: Presence {
                    name: name.into(),
                    color: color.unwrap_or_else(|| colour_for(client)),
                    sheet: 0,
                    selection: [0, 0, 0, 0],
                },
                seen_ms: now_ms,
            },
        );
    }

    /// Record where a participant is looking.
    ///
    /// Takes no identity: that was fixed at join, from the token. An update for
    /// somebody who never joined is ignored rather than inventing them.
    pub fn moved(&mut self, client: ClientId, sheet: usize, selection: [u32; 4], now_ms: u64) {
        if let Some(entry) = self.entries.get_mut(&client) {
            entry.presence.sheet = sheet;
            entry.presence.selection = selection;
            entry.seen_ms = now_ms;
        }
    }

    /// Note that a participant is still connected.
    pub fn heartbeat(&mut self, client: ClientId, now_ms: u64) {
        if let Some(entry) = self.entries.get_mut(&client) {
            entry.seen_ms = now_ms;
        }
    }

    /// Remove a participant that left deliberately.
    pub fn left(&mut self, client: ClientId) {
        self.entries.remove(&client);
    }

    /// Drop everyone unheard for longer than the TTL, returning who went.
    ///
    /// Returned rather than merely removed because the other participants have
    /// to be told: a cursor that stops moving and never disappears is worse
    /// than one that never appeared, since it reads as somebody watching.
    pub fn expire(&mut self, now_ms: u64) -> Vec<ClientId> {
        let ttl = self.ttl_ms;
        let gone: Vec<ClientId> = self
            .entries
            .iter()
            .filter(|(_, entry)| now_ms.saturating_sub(entry.seen_ms) >= ttl)
            .map(|(client, _)| *client)
            .collect();
        for client in &gone {
            self.entries.remove(client);
        }
        gone
    }

    /// Everyone currently present, for a participant that has just joined.
    pub fn everyone(&self) -> impl Iterator<Item = (ClientId, &Presence)> {
        self.entries
            .iter()
            .map(|(client, entry)| (*client, &entry.presence))
    }

    /// How many are here.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the document has no participants — the moment that is a save
    /// point.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A stable colour for a participant that was not given one.
///
/// Deterministic in the id, so the same person is the same colour on every
/// machine and in every session. The palette avoids red, which reads as an
/// error in a spreadsheet, and stays clear of the greens used for validation.
fn colour_for(client: ClientId) -> String {
    const PALETTE: [&str; 8] = [
        "2F6DF6", "7C3AED", "0891B2", "C2410C", "9333EA", "0D9488", "B45309", "4F46E5",
    ];
    PALETTE[(client.0 % PALETTE.len() as u64) as usize].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster {
        Roster::new(30_000)
    }

    #[test]
    fn a_participant_appears_on_joining_and_goes_on_leaving() {
        let mut roster = roster();
        roster.joined(ClientId(1), "Ada", None, 0);
        assert_eq!(roster.len(), 1);
        roster.left(ClientId(1));
        assert!(roster.is_empty());
    }

    #[test]
    fn a_move_carries_no_identity_and_cannot_invent_one() {
        // The rule that matters: presence is where a claimed identity would be
        // believed, so an update for somebody who never joined is ignored.
        let mut roster = roster();
        roster.moved(ClientId(9), 0, [1, 1, 2, 2], 100);
        assert!(roster.is_empty(), "no phantom participant");

        roster.joined(ClientId(9), "Grace", None, 0);
        roster.moved(ClientId(9), 2, [3, 4, 5, 6], 100);
        let (_, presence) = roster.everyone().next().unwrap();
        assert_eq!(presence.name, "Grace", "still the name from the token");
        assert_eq!(presence.sheet, 2);
        assert_eq!(presence.selection, [3, 4, 5, 6]);
    }

    #[test]
    fn silence_expires_a_participant_and_says_who_went() {
        let mut roster = roster();
        roster.joined(ClientId(1), "Ada", None, 0);
        roster.joined(ClientId(2), "Grace", None, 0);

        roster.heartbeat(ClientId(1), 25_000);
        assert!(roster.expire(29_000).is_empty(), "not yet");

        assert_eq!(
            roster.expire(31_000),
            vec![ClientId(2)],
            "the one that stopped talking, and it is reported so others are told"
        );
        assert_eq!(roster.len(), 1);
    }

    #[test]
    fn moving_counts_as_being_alive() {
        let mut roster = roster();
        roster.joined(ClientId(1), "Ada", None, 0);
        roster.moved(ClientId(1), 0, [1, 1, 1, 1], 20_000);
        assert!(
            roster.expire(40_000).is_empty(),
            "a participant that is editing is not silent"
        );
    }

    #[test]
    fn a_colour_depends_on_the_participant_and_nothing_else() {
        // Deliberately different in every way a colour might accidentally
        // depend on: when they joined, who else is present, and in what order.
        // Anything but the id leaking in means somebody changes colour between
        // sessions, which makes the colour identify a cursor rather than a
        // person.
        let mut alone = roster();
        alone.joined(ClientId(42), "Ada", None, 0);

        let mut crowded = roster();
        crowded.joined(ClientId(7), "Grace", None, 500);
        crowded.joined(ClientId(9), "Alan", None, 900);
        crowded.joined(ClientId(42), "Ada", None, 999_999);

        let solo = alone.everyone().next().unwrap().1.color.clone();
        let among_others = crowded
            .everyone()
            .find(|(id, _)| *id == ClientId(42))
            .unwrap()
            .1
            .color
            .clone();
        assert_eq!(solo, among_others, "the same person, so the same colour");

        // And two participants are told apart.
        let grace = crowded
            .everyone()
            .find(|(id, _)| *id == ClientId(7))
            .unwrap()
            .1
            .color
            .clone();
        assert_ne!(grace, among_others, "different people, different colours");
    }

    #[test]
    fn a_supplied_colour_is_used_as_given() {
        let mut roster = roster();
        roster.joined(ClientId(1), "Ada", Some("FF00FF".into()), 0);
        assert_eq!(roster.everyone().next().unwrap().1.color, "FF00FF");
    }

    #[test]
    fn rejoining_replaces_rather_than_duplicates() {
        // Each participant owns exactly one entry and overwrites it wholesale,
        // which is what makes this need no merge and therefore no transform.
        let mut roster = roster();
        roster.joined(ClientId(1), "Ada", None, 0);
        roster.moved(ClientId(1), 3, [9, 9, 9, 9], 10);
        roster.joined(ClientId(1), "Ada", None, 20);

        assert_eq!(roster.len(), 1);
        let (_, presence) = roster.everyone().next().unwrap();
        assert_eq!(
            presence.selection,
            [0, 0, 0, 0],
            "a fresh entry, not a merge"
        );
    }

    #[test]
    fn a_joiner_can_be_told_about_everyone_already_here() {
        let mut roster = roster();
        roster.joined(ClientId(2), "Grace", None, 0);
        roster.joined(ClientId(1), "Ada", None, 0);

        let names: Vec<_> = roster.everyone().map(|(_, p)| p.name.clone()).collect();
        assert_eq!(
            names,
            vec!["Ada".to_owned(), "Grace".to_owned()],
            "in a stable order, so two clients build the same list"
        );
    }
}
